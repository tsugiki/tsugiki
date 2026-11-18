use super::*;
use crate::facts::activity::{Activity, ItemActivity};
use crate::testing::{TempPath, cpython};
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

/// The file of a new database once its migrations have run. Each [`OpenStore::new`] starts from a copy, and
/// skips running them again.
static MIGRATED: OnceLock<Vec<u8>> = OnceLock::new();

impl OpenStore {
    /// A new database in the temp folder `name` names.
    pub(crate) fn new(name: &str) -> Self {
        let migrated = MIGRATED.get_or_init(|| {
            let (path, _folder) = temp_database("migrated");
            Store::open(&path).unwrap().close();
            fs::read(&path).unwrap()
        });
        let (path, folder) = temp_database(name);
        fs::write(&path, migrated).unwrap();
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

    /// Closes the reader and the store, so the folder can go.
    pub(crate) fn close(self) {
        self.open.close();
        self.store.close();
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
        let activity = vec![ItemActivity { number, events }];
        (self.db.store)
            .save_fetched_pull_requests(&cpython(), vec![], activity)
            .unwrap();
    }

    pub(crate) fn marked_seen(&self, number: u32, up_to: OffsetDateTime) {
        (self.db.store)
            .mark_seen(&cpython(), vec![number], Some(up_to))
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
