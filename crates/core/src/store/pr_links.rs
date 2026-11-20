//! The links each PR names, kept as the PR is saved (#833): the numbers it names as fixed, each under the
//! Conventions it counts under, and the Origin PR it names, if it's a Backport. A read takes the rows of the
//! Repository's Conventions, so a change of Conventions regroups the PRs with no rewrite and no sync (ADR
//! 0011). The numbers stay numbers: a read joins them with `issues`, so a number that the sync fetches as an
//! Issue after the PR starts to link then.
//!
//! The rows hold what one version of the link rules gave, `links::RULES_VERSION`, and the store keeps that
//! number beside them. A store that opens with rows from another version keeps them, and answers reads from
//! them until the session has worked each Repository's links out again, a job at a time (#950).

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU32;

use rusqlite::{OptionalExtension, Statement, ToSql, named_params};
use time::OffsetDateTime;

use super::Connection;
use super::columns::{Numbers, UnixTime, text_column};
use super::pull_requests::values_by_number;
use crate::conventions::Conventions;
use crate::facts::CommitSha;
use crate::facts::pull_requests::{PrLinks, PullRequestFacts};
use crate::links::{LinkSignals, RULES_VERSION, fixed_under_each, origins_in_title};
use crate::repository::RepositoryName;

/// The tables of the links, with what finds a PR by the number it names, and what walks the PRs by their
/// last update. A Backport's cherry-pick line names a commit, which a PR became, so the merge commits and
/// the cherry-picked commits are found by their SHAs. `link_rules` holds the version of the rules that
/// worked the rows out, once the store has filled them.
pub(super) const SCHEMA: &str = "
    CREATE TABLE pr_fixes (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        issue INTEGER NOT NULL,
        under TEXT NOT NULL CHECK (under IN ('every', 'cpython', 'github')),
        PRIMARY KEY (repository, number, issue, under),
        FOREIGN KEY (repository, number) REFERENCES pull_requests (repository, number)
    ) WITHOUT ROWID;
    CREATE INDEX pr_fixes_by_issue ON pr_fixes (repository, issue);
    CREATE TABLE pr_origins (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        found_in TEXT NOT NULL CHECK (found_in IN ('cpython', 'cherry_pick')),
        origin INTEGER NOT NULL,
        PRIMARY KEY (repository, number, found_in),
        FOREIGN KEY (repository, number) REFERENCES pull_requests (repository, number)
    ) WITHOUT ROWID;
    CREATE INDEX pr_origins_by_origin ON pr_origins (repository, origin);
    CREATE TABLE link_rules (id INTEGER PRIMARY KEY CHECK (id = 1), version INTEGER NOT NULL);
    CREATE INDEX pull_requests_by_time ON pull_requests (repository, updated_at);
    CREATE INDEX pull_requests_by_merge_commit ON pull_requests (repository, merge_commit);
    CREATE INDEX cherry_picks_by_sha ON cherry_picks (repository, sha);
";

/// The Conventions a kept number counts under, as the `under` column spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Under {
    /// A closing reference or a description's closing keyword, which every Conventions reads.
    Every,
    /// A title's prefix, which only these Conventions read.
    Only(Conventions),
}

text_column!(Under {
    (Every) => "every",
    (Only(Conventions::Cpython)) => "cpython",
    (Only(Conventions::Github)) => "github",
});

/// Where a Backport's Origin PR was named, as the `found_in` column spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FoundIn {
    /// The title's marker under CPython's Conventions.
    CpythonMarker,
    /// A cherry-pick line, whose commit a PR the store holds became.
    CherryPick,
}

text_column!(FoundIn {
    CpythonMarker => "cpython",
    CherryPick => "cherry_pick",
});

impl FoundIn {
    /// Where the title's marker of `conventions` is kept, or nothing for Conventions with no Backports.
    fn marker_of(conventions: Conventions) -> Option<Self> {
        match conventions {
            Conventions::Cpython => Some(Self::CpythonMarker),
            Conventions::Github => None,
        }
    }
}

/// What keeps the links of a Repository's PRs, with its statements prepared once for all the PRs it keeps:
/// those of a save, or every PR the store holds.
pub(super) struct Keeper<'c> {
    repository: &'c RepositoryName,
    forget_fixes: Statement<'c>,
    forget_origins: Statement<'c>,
    forget_picked: Statement<'c>,
    insert_fix: Statement<'c>,
    insert_origin: Statement<'c>,
    picks: Statement<'c>,
    merged: Statement<'c>,
    pickers: Statement<'c>,
}

impl<'c> Keeper<'c> {
    pub(super) fn new(
        connection: &'c Connection,
        repository: &'c RepositoryName,
    ) -> rusqlite::Result<Self> {
        let forget_fixes =
            "DELETE FROM pr_fixes WHERE repository = :repository AND number = :number";
        let forget_origins =
            "DELETE FROM pr_origins WHERE repository = :repository AND number = :number";
        let forget_picked = "DELETE FROM pr_origins
                             WHERE repository = :repository AND number = :number
                                 AND found_in = :found_in";
        let insert_fix = "INSERT OR IGNORE INTO pr_fixes (repository, number, issue, under)
                          VALUES (:repository, :number, :issue, :under)";
        let insert_origin = "INSERT INTO pr_origins (repository, number, found_in, origin)
                             VALUES (:repository, :number, :found_in, :origin)";
        let picks =
            "SELECT sha FROM cherry_picks WHERE repository = :repository AND number = :number
                     ORDER BY sha";
        let merged = "SELECT number, merge_commit FROM pull_requests
                      WHERE repository = :repository AND merge_commit >= :sha
                      ORDER BY merge_commit LIMIT 1";
        let pickers = "SELECT number FROM pr_origins
                       WHERE repository = :repository AND origin = :number AND found_in = :cherry_pick
                       UNION
                       SELECT pick.number AS number FROM json_each(:starts) AS start
                       CROSS JOIN cherry_picks AS pick
                           ON pick.repository = :repository AND pick.sha = start.value";
        Ok(Self {
            repository,
            forget_fixes: connection.prepare(forget_fixes)?,
            forget_origins: connection.prepare(forget_origins)?,
            forget_picked: connection.prepare(forget_picked)?,
            insert_fix: connection.prepare(insert_fix)?,
            insert_origin: connection.prepare(insert_origin)?,
            picks: connection.prepare(picks)?,
            merged: connection.prepare(merged)?,
            pickers: connection.prepare(pickers)?,
        })
    }

    /// Keeps the links of `facts` in place of those kept before. The Origin PR of a Backport whose
    /// cherry-pick line names a commit this PR became, or whose line named this PR before, is found again
    /// too, since the store may have saved the Backport before the PR it came from.
    pub(super) fn rewrite(&mut self, facts: &PullRequestFacts) -> rusqlite::Result<()> {
        let number = facts.number;
        let this = named_params! { ":repository": self.repository, ":number": number };
        self.forget_fixes.execute(this).map(drop)?;
        self.forget_origins.execute(this).map(drop)?;
        self.keep(number, &LinkSignals::of(facts))?;
        self.keep_picked_origin(number)?;
        for picker in self.pickers_of(number, facts.merge_commit.as_ref())? {
            let values = named_params! {
                ":repository": self.repository,
                ":number": picker,
                ":found_in": FoundIn::CherryPick,
            };
            self.forget_picked.execute(values).map(drop)?;
            self.keep_picked_origin(picker)?;
        }
        Ok(())
    }

    /// The PRs whose cherry-pick lines name `merge_commit`, which PR `number` became, whole or cut short,
    /// and those whose lines named PR `number` before.
    fn pickers_of(
        &mut self,
        number: u32,
        merge_commit: Option<&CommitSha>,
    ) -> rusqlite::Result<BTreeSet<u32>> {
        // Each start of the merge commit that a cherry-pick line could name, as a JSON array.
        let starts = merge_commit.map_or_else(Vec::new, |sha| {
            let whole = sha.as_str();
            let lengths = CommitSha::SHORTEST..=whole.len();
            let starts = lengths.filter_map(|length| whole.get(..length));
            starts.map(|start| format!("\"{start}\"")).collect()
        });
        let starts = format!("[{}]", starts.join(","));
        let values = named_params! {
            ":repository": self.repository,
            ":number": number,
            ":cherry_pick": FoundIn::CherryPick,
            ":starts": starts,
        };
        let pickers = self.pickers.query_map(values, |row| row.get("number"))?;
        pickers.collect()
    }

    /// Keeps the numbers `signals` names as fixed and the Origin PRs its title names, for PR `number`,
    /// which has none kept.
    fn keep(&mut self, number: u32, signals: &LinkSignals<'_>) -> rusqlite::Result<()> {
        let repository = self.repository;
        for (issue, conventions) in fixed_under_each(signals, repository) {
            let under = conventions.map_or(Under::Every, Under::Only);
            let values = named_params! {
                ":repository": repository, ":number": number, ":issue": issue, ":under": under,
            };
            self.insert_fix.execute(values).map(drop)?;
        }
        let markers = origins_in_title(signals).into_iter();
        let markers = markers
            .filter_map(|(conventions, origin)| Some((FoundIn::marker_of(conventions)?, origin)));
        for (found_in, origin) in markers {
            self.save_origin(number, found_in, origin)?;
        }
        Ok(())
    }

    /// Keeps `origin` as the Origin PR that PR `number` names in `found_in`.
    fn save_origin(&mut self, number: u32, found_in: FoundIn, origin: u32) -> rusqlite::Result<()> {
        let values = named_params! {
            ":repository": self.repository,
            ":number": number,
            ":found_in": found_in,
            ":origin": origin,
        };
        self.insert_origin.execute(values).map(drop)
    }

    /// Keeps the Origin PR that PR `number`'s cherry-pick lines name, which has none kept: the PR that the
    /// first of its commits that a PR became, in the order of their SHAs, became. A short SHA names the
    /// first whole one it starts, as `MergeCommits` reads it.
    fn keep_picked_origin(&mut self, number: u32) -> rusqlite::Result<()> {
        let this = named_params! { ":repository": self.repository, ":number": number };
        let shas = self
            .picks
            .query_map(this, |row| row.get::<_, CommitSha>("sha"))?;
        let shas: Vec<CommitSha> = shas.collect::<rusqlite::Result<_>>()?;
        for sha in shas {
            let at = named_params! { ":repository": self.repository, ":sha": sha };
            let found = self.merged.query_row(at, |row| {
                let whole: CommitSha = row.get("merge_commit")?;
                Ok((row.get::<_, u32>("number")?, whole))
            });
            let Some((origin, whole)) = found.optional()? else {
                continue;
            };
            if whole.starts_with(&sha) {
                return self.save_origin(number, FoundIn::CherryPick, origin);
            }
        }
        Ok(())
    }
}

/// Saves this version of the rules as the one that worked out the kept links.
fn keep_version(connection: &Connection) -> rusqlite::Result<()> {
    let query = "INSERT INTO link_rules (id, version) VALUES (1, :version)
                 ON CONFLICT (id) DO UPDATE SET version = excluded.version";
    let version = named_params! { ":version": RULES_VERSION };
    connection.execute(query, version).map(drop)
}

/// The Repository a name the store holds spells, as a refill reads its rows by: nothing for a name that no
/// longer parses, or that parses to another spelling than its own, as one with a URL's prefix or `.git`
/// would. A name parsed once, so these come only from a store an older Tsugiki or a hand edit wrote.
fn spelled(name: &str) -> Option<RepositoryName> {
    let repository = RepositoryName::parse(name).ok()?;
    (repository.to_string())
        .eq_ignore_ascii_case(name)
        .then_some(repository)
}

/// Saves this version of the rules as the one that worked out the kept links, once the session has refilled
/// each Repository `stale` named: a PR saved meanwhile kept links under this version's rules. The links of a
/// name that spells no Repository go, as no refill reaches them.
pub(super) fn refilled(connection: &mut Connection) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    let query = "SELECT repository FROM pr_fixes UNION SELECT repository FROM pr_origins";
    let names: Vec<String> = {
        let mut statement = transaction.prepare(query)?;
        let names = statement.query_map([], |row| row.get("repository"))?;
        names.collect::<rusqlite::Result<_>>()?
    };
    let unread = names.iter().filter(|name| spelled(name).is_none());
    for name in unread {
        for table in ["pr_fixes", "pr_origins"] {
            let forget = format!("DELETE FROM {table} WHERE repository = :repository");
            transaction
                .execute(&forget, named_params! { ":repository": name })
                .map(drop)?;
        }
    }
    keep_version(&transaction)?;
    transaction.commit()
}

/// Saves this version of the rules as the one that worked out the kept links, in a store that holds no PR,
/// whose links any version's rules leave empty: a new file, or one the step that makes the tables found
/// empty. A store that holds PRs keeps the version it has.
pub(super) fn keep_version_if_empty(connection: &mut Connection) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    let empty = "SELECT NOT EXISTS (SELECT 1 FROM pull_requests)";
    if transaction.query_row(empty, [], |row| row.get::<_, bool>(0))? {
        keep_version(&transaction)?;
    }
    transaction.commit()
}

/// The Repositories whose PRs' links to work out again, unless the rules that worked out the kept ones are
/// this version's: as the step that makes the tables leaves them empty, or after a release changes the rules.
/// Each Repository the store holds PRs of is among them.
pub(super) fn stale(connection: &Connection) -> rusqlite::Result<Option<Vec<RepositoryName>>> {
    let kept: Option<u32> = connection
        .query_row("SELECT version FROM link_rules", [], |row| {
            row.get("version")
        })
        .optional()?;
    if kept == Some(RULES_VERSION) {
        return Ok(None);
    }
    let mut statement = connection.prepare("SELECT DISTINCT repository FROM pull_requests")?;
    let names = statement.query_map([], |row| row.get::<_, String>("repository"))?;
    let names: Vec<String> = names.collect::<rusqlite::Result<_>>()?;
    // A name that spells no Repository has nothing a refill reads by, and `refilled` forgets its links.
    Ok(Some(
        names.iter().filter_map(|name| spelled(name)).collect(),
    ))
}

/// What a PR's links are worked out from, as the store holds it.
struct HeldSignals {
    number: u32,
    title: String,
    off_default_description: Option<String>,
}

impl HeldSignals {
    /// What `row` of `pull_requests` holds of them.
    fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            number: row.get("number")?,
            title: row.get("title")?,
            off_default_description: row.get("off_default_description")?,
        })
    }
}

/// Keeps the links of the first `most` PRs of `repository` from number `from` up that the store holds, in
/// place of those kept, in one transaction. Answers with the number the next batch starts from, or nothing
/// once no PR is left. It reads only what the rules read, and finds each Backport's Origin PR once, since
/// every PR of the Repository is there to be found.
pub(super) fn refill(
    connection: &mut Connection,
    repository: &RepositoryName,
    from: u32,
    most: NonZeroU32,
) -> rusqlite::Result<Option<u32>> {
    let transaction = connection.transaction()?;
    let query = "SELECT number, title, off_default_description FROM pull_requests
                 WHERE repository = :repository AND number >= :from ORDER BY number LIMIT :most";
    let asked = named_params! { ":repository": repository, ":from": from, ":most": most.get() };
    let held: Vec<HeldSignals> = {
        let mut statement = transaction.prepare(query)?;
        let rows = statement.query_map(asked, HeldSignals::read)?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let Some(last) = held.last().map(|pr| pr.number) else {
        return Ok(None);
    };
    let these: [(&str, &dyn ToSql); 3] = [
        (":repository", repository),
        (":from", &from),
        (":last", &last),
    ];
    for table in ["pr_fixes", "pr_origins"] {
        let forget = format!(
            "DELETE FROM {table} WHERE repository = :repository AND number BETWEEN :from AND :last"
        );
        transaction.execute(&forget, &these[..]).map(drop)?;
    }
    let closing = "SELECT number, issue FROM closing_references
                   WHERE repository = :repository AND number BETWEEN :from AND :last
                   ORDER BY number, issue";
    let closing: BTreeMap<u32, Vec<u32>> =
        values_by_number(&mut transaction.prepare(closing)?, &these, "issue")?;
    {
        let mut keeper = Keeper::new(&transaction, repository)?;
        for pr in &held {
            let signals = LinkSignals {
                title: &pr.title,
                closing_issues: closing.get(&pr.number).map_or(&[][..], Vec::as_slice),
                off_default_description: pr.off_default_description.as_deref(),
            };
            keeper.keep(pr.number, &signals)?;
            keeper.keep_picked_origin(pr.number)?;
        }
    }
    transaction.commit()?;
    let full = held.len() == usize::try_from(most.get()).unwrap_or(usize::MAX);
    Ok(if full { last.checked_add(1) } else { None })
}

/// The condition on `pr_origins` as `origin` that keeps the Origin PRs `conventions` read, with what it
/// binds: its title's marker, then a cherry-pick line. Nothing, for Conventions with no Backports.
fn origins_read_under(conventions: Conventions) -> Option<(&'static str, FoundIn)> {
    let marker = FoundIn::marker_of(conventions)?;
    Some(("origin.found_in IN (:marker, :cherry_pick)", marker))
}

/// The links of each PR of `numbers` that the store holds, under `conventions`, in number order.
pub(super) fn read(
    connection: &Connection,
    repository: &RepositoryName,
    conventions: Conventions,
    numbers: &Numbers,
) -> rusqlite::Result<Vec<PrLinks>> {
    let these: [(&str, &dyn ToSql); 2] = [(":repository", repository), (":numbers", numbers)];
    let own = Under::Only(conventions);
    let fixes_query = "SELECT fix.number AS number, fix.issue AS issue FROM pr_fixes AS fix
         WHERE fix.repository = :repository
             AND fix.number IN (SELECT value FROM json_each(:numbers))
             AND fix.under IN (:every, :own)
             AND EXISTS (SELECT 1 FROM issues AS issue
                         WHERE issue.repository = fix.repository AND issue.number = fix.issue)
         ORDER BY fix.number, fix.issue";
    let under: [(&str, &dyn ToSql); 2] = [(":every", &Under::Every), (":own", &own)];
    let asked = [&these[..], &under].concat();
    let mut fixes: BTreeMap<u32, Vec<u32>> =
        values_by_number(&mut connection.prepare(fixes_query)?, &asked, "issue")?;
    let mut origins: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    if let Some((condition, marker)) = origins_read_under(conventions) {
        // A title's marker sorts before a cherry-pick line, and comes first (ADR 0011).
        let origins_query = format!(
            "SELECT origin.number AS number, origin.origin AS origin FROM pr_origins AS origin
             WHERE origin.repository = :repository
                 AND origin.number IN (SELECT value FROM json_each(:numbers)) AND {condition}
             ORDER BY origin.number, origin.found_in = :cherry_pick"
        );
        let found_in: [(&str, &dyn ToSql); 2] =
            [(":marker", &marker), (":cherry_pick", &FoundIn::CherryPick)];
        let asked = [&these[..], &found_in].concat();
        let mut statement = connection.prepare(&origins_query)?;
        origins = values_by_number(&mut statement, &asked, "origin")?;
    }
    let query = "SELECT number, state, updated_at FROM pull_requests
                 WHERE repository = :repository AND number IN (SELECT value FROM json_each(:numbers))
                 ORDER BY number";
    let mut statement = connection.prepare(query)?;
    let mut rows = statement.query(&these[..])?;
    let mut found = Vec::new();
    while let Some(row) = rows.next()? {
        let number: u32 = row.get("number")?;
        let UnixTime(updated_at) = row.get("updated_at")?;
        found.push(PrLinks {
            number,
            state: row.get("state")?,
            updated_at,
            fixes: fixes
                .remove(&number)
                .unwrap_or_default()
                .into_iter()
                .collect(),
            origin: origins
                .remove(&number)
                .and_then(|named| named.first().copied()),
        });
    }
    Ok(found)
}

/// The PRs that fix one of `issues` under `conventions`.
pub(super) fn fixing(
    connection: &Connection,
    repository: &RepositoryName,
    conventions: Conventions,
    issues: &Numbers,
) -> rusqlite::Result<BTreeSet<u32>> {
    // Each listed Issue finds its PRs through `pr_fixes_by_issue`, which `CROSS JOIN` makes SQLite take.
    let query = "SELECT fix.number AS number FROM json_each(:issues) AS listed
                 CROSS JOIN pr_fixes AS fix ON fix.repository = :repository AND fix.issue = listed.value
                 WHERE fix.under IN (:every, :own)";
    let own = Under::Only(conventions);
    let asked: [(&str, &dyn ToSql); 4] = [
        (":repository", repository),
        (":issues", issues),
        (":every", &Under::Every),
        (":own", &own),
    ];
    let mut statement = connection.prepare(query)?;
    let found = statement.query_map(&asked[..], |row| row.get("number"))?;
    found.collect()
}

/// The PRs that name one of `origins` as their Origin PR under `conventions`. A Backport whose title names
/// another Origin PR than its cherry-pick line is among them, and its links say which comes first.
pub(super) fn naming_origins(
    connection: &Connection,
    repository: &RepositoryName,
    conventions: Conventions,
    origins: &Numbers,
) -> rusqlite::Result<BTreeSet<u32>> {
    let Some((condition, marker)) = origins_read_under(conventions) else {
        return Ok(BTreeSet::new());
    };
    // Each listed PR finds what names it through `pr_origins_by_origin`, as `fixing` does.
    let query = format!(
        "SELECT origin.number AS number FROM json_each(:origins) AS listed
         CROSS JOIN pr_origins AS origin
             ON origin.repository = :repository AND origin.origin = listed.value
         WHERE {condition}"
    );
    let asked: [(&str, &dyn ToSql); 4] = [
        (":repository", repository),
        (":origins", origins),
        (":marker", &marker),
        (":cherry_pick", &FoundIn::CherryPick),
    ];
    let mut statement = connection.prepare(&query)?;
    let found = statement.query_map(&asked[..], |row| row.get("number"))?;
    found.collect()
}

/// The updates a walk down the cache reads, the latest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Updates {
    /// Those at `since` or later.
    Since(OffsetDateTime),
    /// The `at_least` latest before `before`, and every other in the second of the last of them, so the
    /// next walk can start before that second. Fewer means none are left.
    Before {
        before: OffsetDateTime,
        at_least: u32,
    },
}

impl Updates {
    /// The condition on `updated_at` that keeps these updates in `table`, where `kept` are the other
    /// conditions of the `WHERE`, and the parameters it names, beside those of `kept`.
    pub(super) fn condition(self, table: &str, kept: &str) -> (String, Vec<(&'static str, i64)>) {
        match self {
            Self::Since(since) => (
                "AND updated_at >= :since".to_owned(),
                vec![(":since", since.unix_timestamp())],
            ),
            Self::Before { before, at_least } => {
                let condition = format!(
                    "AND updated_at < :before AND updated_at >= IFNULL(
                         (SELECT updated_at FROM {table} WHERE {kept} AND updated_at < :before
                          ORDER BY updated_at DESC LIMIT 1 OFFSET :skip),
                         :lowest)"
                );
                let skip = i64::from(at_least.saturating_sub(1));
                let asked = vec![
                    (":before", before.unix_timestamp()),
                    (":skip", skip),
                    (":lowest", i64::MIN),
                ];
                (condition, asked)
            }
        }
    }
}

/// `times`, which `Updates::condition` names, as parameters of a query.
pub(super) fn bound<'a>(
    times: &'a [(&'static str, i64)],
) -> impl Iterator<Item = (&'a str, &'a dyn ToSql)> {
    times
        .iter()
        .map(|(name, time)| -> (&str, &dyn ToSql) { (name, time) })
}

/// Each PR that `updates` takes, whatever its state, with its last update, the latest first.
pub(super) fn updated(
    connection: &Connection,
    repository: &RepositoryName,
    updates: Updates,
) -> rusqlite::Result<Vec<(u32, OffsetDateTime)>> {
    let kept = "repository = :repository";
    let (condition, times) = updates.condition("pull_requests", kept);
    let query = format!(
        "SELECT number, updated_at FROM pull_requests WHERE {kept} {condition}
         ORDER BY updated_at DESC, number DESC"
    );
    let mut asked: Vec<(&str, &dyn ToSql)> = vec![(":repository", repository)];
    asked.extend(bound(&times));
    let mut statement = connection.prepare(&query)?;
    let mut rows = statement.query(asked.as_slice())?;
    let mut found = Vec::new();
    while let Some(row) = rows.next()? {
        let UnixTime(updated_at) = row.get("updated_at")?;
        found.push((row.get("number")?, updated_at));
    }
    Ok(found)
}

#[cfg(test)]
mod tests;
