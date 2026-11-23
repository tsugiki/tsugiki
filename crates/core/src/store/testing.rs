use super::*;
use crate::facts::activity::{Activity, ItemActivity};
use crate::links::LinkRule;
use crate::testing::{TempPath, cpython, gcmon};
use std::path::PathBuf;
use std::sync::OnceLock;

pub(super) const V1: Migration = Migration {
    version: 1,
    sql: "CREATE TABLE notes (id INTEGER PRIMARY KEY, text TEXT NOT NULL);
              INSERT INTO notes (text) VALUES ('kept');",
};
pub(super) const V2: Migration = Migration {
    version: 2,
    sql: "CREATE TABLE tags (id INTEGER PRIMARY KEY);",
};
pub(super) const V3: Migration = Migration {
    version: 3,
    sql: "CREATE TABLE labels (id INTEGER PRIMARY KEY);",
};
pub(super) const V3_BROKEN: Migration = Migration {
    version: 3,
    sql: "CREATE TABLE half_done (id INTEGER PRIMARY KEY); THIS IS NOT SQL;",
};

/// The path of a database in a new temp folder, and that folder. Close the database before the folder drops.
pub(super) fn temp_database(name: &str) -> (PathBuf, TempPath) {
    let folder = crate::testing::temp_path(&format!("store-{name}"));
    fs::create_dir_all(&folder).unwrap();
    (folder.join("tsugiki.db"), folder)
}

/// The file of a new database once its migrations have run. Each [`migrated_database`] starts from a copy,
/// and skips running them again.
static MIGRATED: OnceLock<Vec<u8>> = OnceLock::new();

/// A database in a new temp folder, as [`temp_database`] makes it, with every migration run on it, and that
/// folder. Close the database before the folder drops.
pub(super) fn migrated_database(name: &str) -> (PathBuf, TempPath) {
    let migrated = MIGRATED.get_or_init(|| {
        let (path, _folder) = temp_database("migrated");
        Store::open(&path).unwrap().close();
        fs::read(&path).unwrap()
    });
    let (path, folder) = temp_database(name);
    fs::write(&path, migrated).unwrap();
    (path, folder)
}

/// Archives the items of CPython that `keys` name as a sync does when it sees them close, put away `now`.
pub(crate) fn archived_on_closing(store: &Store, keys: &[ItemKey], now: OffsetDateTime) {
    let closed = keys.to_vec();
    let keep = move |connection: &mut Connection| {
        let closed_on =
            |key| activity::follow_state(connection, &cpython(), key, activity::StateMove::Closed);
        closed.into_iter().try_for_each(closed_on)
    };
    store.call(keep).unwrap().unwrap();
    let judged = Judged {
        archived: keys.to_vec(),
        closings: keys.iter().map(|key| key.number()).collect(),
    };
    store.archive_closed(&cpython(), judged, now).unwrap();
}

/// The Pinned issues of `repository` (#488).
pub(crate) fn pinned(store: &Store, repository: &RepositoryName) -> BTreeSet<u32> {
    store.pins(repository).unwrap().into_keys().collect()
}

/// The pins of `repository` whose trees no sync has read since the pin (#488).
pub(crate) fn pins_to_read(store: &Store, repository: &RepositoryName) -> BTreeMap<u32, PinId> {
    let pins = store.pins(repository).unwrap().into_iter();
    let to_read = pins.filter_map(|(number, tree)| match tree {
        PinTree::ToRead(pin) => Some((number, pin)),
        PinTree::Read => None,
    });
    to_read.collect()
}

/// Unpins Issue `number` of `repository` (#488).
pub(crate) fn unpin(store: &Store, repository: &RepositoryName, number: u32) {
    store
        .unpin(repository, number, OffsetDateTime::now_utc())
        .unwrap();
}

/// Mark Seen on `numbers` up to `up_to`, with no Stale review drawn.
pub(crate) fn seeing(numbers: Vec<u32>, up_to: Option<OffsetDateTime>) -> Seeing {
    Seeing {
        numbers,
        up_to,
        findings: vec![],
    }
}

/// The text each of `all` leaves in a column, once it has read back as itself.
pub(super) fn stored_texts<T>(all: &[T]) -> Vec<String>
where
    T: rusqlite::ToSql + rusqlite::types::FromSql + std::fmt::Debug + PartialEq,
{
    let connection = Connection::open_in_memory().unwrap();
    let through = |value: &T| {
        let (text, read): (String, T) = connection
            .query_row(
                "SELECT :value AS value",
                named_params! { ":value": value },
                |row| Ok((row.get("value")?, row.get("value")?)),
            )
            .unwrap();
        assert_eq!(&read, value);
        text
    };
    all.iter().map(through).collect()
}

/// The rules an older Tsugiki's links name in `store_with_old_links`: the first set, less its last rule.
pub(super) const OLDER_RULES: &[LinkRule] = &[
    LinkRule::ClosingReferences,
    LinkRule::DescriptionKeywords,
    LinkRule::CpythonTitleIssues,
    LinkRule::CpythonTitleOrigin,
];

/// Adds `name` to the rules `store` says worked out its links, as a newer Tsugiki would keep a rule this
/// build doesn't know.
pub(super) fn name_a_newer_rule(store: &Store, name: &'static str) {
    let named = store.call(move |connection| {
        let keep = "INSERT INTO link_rules (name) VALUES (:name)";
        connection
            .execute(keep, named_params! { ":name": name })
            .map(drop)
    });
    named.unwrap().unwrap();
}

/// Leaves `store` with the links `rules` worked out, as another Tsugiki kept them: only `fixes`, each a
/// Repository, a PR and the Issue it fixed under every Conventions, which a test tells from what
/// `LinkRule::SET` gives.
pub(crate) fn keep_old_links(
    store: &Store,
    rules: &[LinkRule],
    fixes: &[(RepositoryName, u32, u32)],
) {
    let fixes = fixes.to_vec();
    let rules = rules.to_vec();
    let kept = store.call(move |connection| -> rusqlite::Result<()> {
        let transaction = connection.transaction()?;
        transaction.execute_batch("DELETE FROM pr_fixes; DELETE FROM pr_origins;")?;
        for (repository, number, issue) in &fixes {
            let insert = "INSERT INTO pr_fixes (repository, number, issue, under)
                          VALUES (:repository, :number, :issue, 'every')";
            let values = named_params! {
                ":repository": repository, ":number": number, ":issue": issue,
            };
            transaction.execute(insert, values).map(drop)?;
        }
        transaction
            .execute("DELETE FROM link_rules", [])
            .map(drop)?;
        for rule in &rules {
            let keep = "INSERT INTO link_rules (name) VALUES (:name)";
            transaction
                .execute(keep, named_params! { ":name": rule })
                .map(drop)?;
        }
        transaction.commit()
    });
    kept.unwrap().unwrap();
}

/// A store in memory that holds PR 10 of each of `repositories`, titled to fix Issue 5, with the links an
/// older set of rules kept, which lacks a rule of `LinkRule::SET`: each PR fixes Issue 6.
pub(crate) fn store_with_old_links(repositories: &[RepositoryName]) -> Store {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let fixing = PullRequestFacts {
        title: "gh-5: Fix it".to_owned(),
        ..crate::testing::pr_facts(10)
    };
    for repository in repositories {
        (store.save_pull_requests(repository, vec![fixing.clone()])).unwrap();
    }
    let old: Vec<_> = repositories
        .iter()
        .map(|name| (name.clone(), 10, 6))
        .collect();
    keep_old_links(&store, OLDER_RULES, &old);
    store
}

/// Has `store` refuse each link it would keep from now on, as a full disk refuses a refill's rows.
pub(crate) fn refuse_new_links(store: &Store) {
    let refusing = store.call(|connection| {
        connection.execute_batch(
            "CREATE TRIGGER refuse_fixes BEFORE INSERT ON pr_fixes BEGIN SELECT RAISE(ABORT, 'full'); END;",
        )
    });
    refusing.unwrap().unwrap();
}

/// Has `store` refuse each new Read-only mark, as a full disk would.
pub(crate) fn refuse_read_only_marks(store: &Store) {
    let refusing = store.call(|connection| {
        connection.execute_batch(
            "CREATE TRIGGER refuse_marks BEFORE INSERT ON read_only BEGIN SELECT RAISE(ABORT, 'full'); END;",
        )
    });
    refusing.unwrap().unwrap();
}

/// Each fix `store` keeps, as its Repository, its PR and the Issue the PR fixes, in that order.
pub(crate) fn kept_fixes(store: &Store) -> Vec<(String, u32, u32)> {
    let found = store.call(|connection| -> rusqlite::Result<Vec<(String, u32, u32)>> {
        let query =
            "SELECT repository, number, issue FROM pr_fixes ORDER BY repository, number, issue";
        let mut statement = connection.prepare(query)?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        rows.collect()
    });
    found.unwrap().unwrap()
}

/// The moment `seconds` after 1970, as a GitHub `updatedAt` a test orders reads by.
pub(super) fn at_second(seconds: i64) -> OffsetDateTime {
    OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(seconds)
}

/// A database in a new temp folder, as [`temp_database`] makes it, at the age before the migration `sql`,
/// holding what the SQL statements `rows` insert: a store of that age can't run today's saves.
pub(super) fn database_before(name: &str, sql: &str, rows: &str) -> (PathBuf, TempPath) {
    let (path, folder) = temp_database(name);
    let before = MIGRATIONS.iter().position(|step| step.sql == sql).unwrap();
    open_with(&path, &MIGRATIONS[..before]).unwrap().close();
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch(rows).unwrap();
    // A seed that inserts nothing would leave the test reading an empty store.
    assert!(connection.total_changes() > 0, "{rows} inserted no row");
    (path, folder)
}

/// A database in a new temp folder, the store that writes it and a reader of it.
pub(crate) struct OpenStore {
    pub(crate) store: Store,
    pub(crate) reader: StoreReader,
    pub(crate) path: PathBuf,
    pub(crate) open: OpenReader,
    /// Dropped last, once `close` has closed the database in it.
    _folder: TempPath,
}

impl OpenStore {
    /// A new database in the temp folder `name` names.
    pub(crate) fn new(name: &str) -> Self {
        let (path, folder) = migrated_database(name);
        Self::of(path, folder)
    }

    /// The database at `path` in the temp `folder`, which a test has written to before.
    pub(crate) fn of(path: PathBuf, folder: TempPath) -> Self {
        let store = Store::open(&path).unwrap();
        let (to_open, reader) = reader();
        let open = to_open.open(&path).unwrap();
        Self {
            store,
            reader,
            path,
            open,
            _folder: folder,
        }
    }

    /// Whether the store marks CPython and gcmon Read-only for the token (#398).
    pub(crate) fn read_only_marks(&self) -> (bool, bool) {
        let marked = |repository| self.reader.token_cant_write(&repository).unwrap();
        (marked(cpython()), marked(gcmon()))
    }

    /// Each of `events` happened on CPython's Issue or PR `number`, as a fetch saves them.
    pub(crate) fn happened(&self, number: u32, events: Vec<Activity>) {
        let activity = vec![ItemActivity { number, events }];
        (self.store)
            .save_fetched_pull_requests(&cpython(), vec![], activity)
            .unwrap();
    }

    /// Closes the reader and the store, so the folder can go.
    pub(crate) fn close(self) {
        self.open.close();
        self.store.close();
    }
}

impl StoreReader {
    /// What Issue `number` says, once Tsugiki has read it, as the Issue view reads it.
    pub(crate) fn issue_content(
        &self,
        repository: &RepositoryName,
        number: u32,
    ) -> Result<Option<IssueContent>, Error> {
        self.read_snapshot(repository, move |snapshot| snapshot.issue_content(number))
    }

    /// PR `number`'s Conversation, once Tsugiki has read it, whether or not the store holds the PR's facts.
    pub(crate) fn pull_request_content(
        &self,
        repository: &RepositoryName,
        number: u32,
    ) -> Result<Option<PullRequestContent>, Error> {
        self.read_snapshot(repository, move |snapshot| {
            snapshot.pull_request_content(number)
        })
    }
}

/// A database of `cpython` whose user signed in as `me()` and whose first sync began three days ago: the
/// world of the scenarios of New activity and Needs attention. Each scenario's file adds what it reads.
pub(crate) struct World {
    pub(crate) db: OpenStore,
}

impl World {
    /// The world in the temp folder `name` names, once the first sync has finished.
    pub(crate) fn new(name: &str) -> Self {
        let world = Self::before_its_first_sync(name);
        world.finishes_its_first_sync();
        world
    }

    pub(crate) fn before_its_first_sync(name: &str) -> Self {
        let db = OpenStore::new(name);
        db.store.remember_login(&crate::testing::me()).unwrap();
        Self { db }
    }

    pub(crate) fn finishes_its_first_sync(&self) {
        let started_at = crate::testing::hours_ago(72);
        let state = SyncState {
            started_at,
            finished_at: started_at,
            check: ChangeCheck::Skipped,
            refresh_under_way: false,
        };
        self.db.store.save_sync_state(&cpython(), state).unwrap();
    }

    pub(crate) fn has(&self, pull_requests: Vec<PullRequestFacts>) {
        (self.db.store)
            .save_pull_requests(&cpython(), pull_requests)
            .unwrap();
    }

    pub(crate) fn happened(&self, number: u32, events: Vec<Activity>) {
        self.db.happened(number, events);
    }

    pub(crate) fn marked_seen(&self, number: u32, up_to: OffsetDateTime) {
        (self.db.store)
            .mark_seen(&cpython(), seeing(vec![number], Some(up_to)))
            .unwrap();
    }

    pub(crate) fn finish(self) {
        self.db.close();
    }
}

pub(super) fn user_version(path: &Path) -> u32 {
    Connection::open(path)
        .unwrap()
        .pragma_query_value(None, "user_version", |row| row.get("user_version"))
        .unwrap()
}

pub(super) fn has_table(path: &Path, table: &str) -> bool {
    Connection::open(path)
        .unwrap()
        .prepare(&format!("SELECT 1 FROM {table}"))
        .is_ok()
}

/// A database in memory with every step of the schema run on it.
pub(super) fn migrated() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    for step in super::migrate::MIGRATIONS {
        connection.execute_batch(step.sql).unwrap();
    }
    connection
}

pub(super) fn backups(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".bak"))
        .collect();
    names.sort();
    names
}
