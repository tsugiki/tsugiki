//! The links each PR names, kept as the PR is saved (#833): the numbers it names as fixed, each under the
//! Conventions it counts under, and the Origin PR it names, if it's a Backport. A read takes the rows of the
//! Repository's Conventions, so a change of Conventions regroups the PRs with no rewrite and no sync (ADR
//! 0011). The numbers stay numbers: a read joins them with `issues`, so a number that the sync fetches as an
//! Issue after the PR starts to link then.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::types::ToSqlOutput;
use rusqlite::{ToSql, named_params};
use time::OffsetDateTime;

use super::Connection;
use super::columns::{Numbers, UnixTime};
use super::pull_requests::{self, values_by_number};
use crate::conventions::Conventions;
use crate::facts::CommitSha;
use crate::facts::pull_requests::{PrLinks, PullRequestFacts};
use crate::links::{fixed_under_each, origins_in_title};
use crate::repository::RepositoryName;

/// The tables of the links, with what finds a PR by the number it names, and what walks the PRs by their
/// last update. A Backport's cherry-pick line names a commit, which a PR became, so the merge commits and
/// the cherry-picked commits are found by their SHAs.
pub(super) const SCHEMA: &str = "
    CREATE TABLE pr_fixes (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        issue INTEGER NOT NULL,
        under TEXT NOT NULL,
        PRIMARY KEY (repository, number, issue, under),
        FOREIGN KEY (repository, number) REFERENCES pull_requests (repository, number)
    ) WITHOUT ROWID;
    CREATE INDEX pr_fixes_by_issue ON pr_fixes (repository, issue);
    CREATE TABLE pr_origins (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        found_in TEXT NOT NULL,
        origin INTEGER NOT NULL,
        PRIMARY KEY (repository, number, found_in),
        FOREIGN KEY (repository, number) REFERENCES pull_requests (repository, number)
    ) WITHOUT ROWID;
    CREATE INDEX pr_origins_by_origin ON pr_origins (repository, origin);
    CREATE INDEX pull_requests_by_time ON pull_requests (repository, updated_at);
    CREATE INDEX pull_requests_by_merge_commit ON pull_requests (repository, merge_commit);
    CREATE INDEX cherry_picks_by_sha ON cherry_picks (repository, sha);
";

/// The Conventions a kept number counts under, as the `under` column spells it.
#[derive(Clone, Copy)]
enum Under {
    /// A closing reference or a description's closing keyword, which every Conventions reads.
    Every,
    /// A title's prefix, which only these Conventions read.
    Only(Conventions),
}

/// Where a Backport's Origin PR was named, as the `found_in` column spells it.
#[derive(Clone, Copy)]
enum FoundIn {
    /// The title's marker, under these Conventions.
    Title(Conventions),
    /// A cherry-pick line, whose commit a PR the store holds became.
    CherryPick,
}

fn spelled(conventions: Conventions) -> &'static str {
    match conventions {
        Conventions::Cpython => "cpython",
        Conventions::Github => "github",
    }
}

impl ToSql for Under {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(match self {
            Self::Every => "every",
            Self::Only(conventions) => spelled(*conventions),
        }))
    }
}

impl ToSql for FoundIn {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(match self {
            Self::Title(conventions) => spelled(*conventions),
            Self::CherryPick => "cherry_pick",
        }))
    }
}

/// The parameters that pick the rows of one Conventions: the numbers every Conventions counts and those of
/// its own titles, and the Origin PR of its title's marker, or of a cherry-pick line if it has Backports.
struct Picked {
    every: Under,
    own: Under,
    title: FoundIn,
    cherry_pick: FoundIn,
    backports: bool,
}

impl Picked {
    fn of(conventions: Conventions) -> Self {
        Self {
            every: Under::Every,
            own: Under::Only(conventions),
            title: FoundIn::Title(conventions),
            cherry_pick: FoundIn::CherryPick,
            backports: conventions.has_backports(),
        }
    }

    /// The condition on `pr_fixes` as `fix` that keeps the numbers these Conventions count.
    const FIXES: &'static str = "fix.under IN (:every, :own)";

    /// The condition on `pr_origins` as `origin` that keeps the Origin PRs these Conventions read.
    const ORIGINS: &'static str =
        "(origin.found_in = :title OR (origin.found_in = :cherry_pick AND :backports))";

    fn fixes(&self) -> [(&str, &dyn ToSql); 2] {
        [(":every", &self.every), (":own", &self.own)]
    }

    fn origins(&self) -> [(&str, &dyn ToSql); 3] {
        [
            (":title", &self.title),
            (":cherry_pick", &self.cherry_pick),
            (":backports", &self.backports),
        ]
    }
}

/// Keeps the links of `facts` in place of those kept before. The Origin PR of a Backport whose cherry-pick
/// line names a commit this PR became, or whose line named this PR before, is found again too, since the
/// store may have saved the Backport before the PR it came from.
pub(super) fn rewrite(
    connection: &Connection,
    repository: &RepositoryName,
    facts: &PullRequestFacts,
) -> rusqlite::Result<()> {
    let number = facts.number;
    let this = named_params! { ":repository": repository, ":number": number };
    let forget_fixes = "DELETE FROM pr_fixes WHERE repository = :repository AND number = :number";
    connection.execute(forget_fixes, this).map(drop)?;
    let forget_origins =
        "DELETE FROM pr_origins WHERE repository = :repository AND number = :number";
    connection.execute(forget_origins, this).map(drop)?;
    for (issue, conventions) in fixed_under_each(facts, repository) {
        let under = conventions.map_or(Under::Every, Under::Only);
        let query = "INSERT OR IGNORE INTO pr_fixes (repository, number, issue, under)
                     VALUES (:repository, :number, :issue, :under)";
        let values = named_params! {
            ":repository": repository, ":number": number, ":issue": issue, ":under": under,
        };
        connection.execute(query, values).map(drop)?;
    }
    for (conventions, origin) in origins_in_title(facts) {
        let found_in = FoundIn::Title(conventions);
        save_origin(connection, (repository, number), found_in, origin)?;
    }
    find_picked_origin(connection, repository, number)?;
    for picker in pickers_of(connection, repository, number, facts.merge_commit.as_ref())? {
        find_picked_origin(connection, repository, picker)?;
    }
    Ok(())
}

/// Keeps `origin` as the Origin PR that PR `number` of `repository` names in `found_in`.
fn save_origin(
    connection: &Connection,
    (repository, number): (&RepositoryName, u32),
    found_in: FoundIn,
    origin: u32,
) -> rusqlite::Result<()> {
    let query = "INSERT INTO pr_origins (repository, number, found_in, origin)
                 VALUES (:repository, :number, :found_in, :origin)";
    let values = named_params! {
        ":repository": repository, ":number": number, ":found_in": found_in, ":origin": origin,
    };
    connection.execute(query, values).map(drop)
}

/// Keeps the Origin PR that PR `number`'s cherry-pick lines name, in place of the one kept before: the PR
/// that the first of its commits that a PR became, in the order of their SHAs, became. A short SHA names the
/// first whole one it starts, as `MergeCommits` reads it.
fn find_picked_origin(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
) -> rusqlite::Result<()> {
    let picked = FoundIn::CherryPick;
    let forget = "DELETE FROM pr_origins
                  WHERE repository = :repository AND number = :number AND found_in = :found_in";
    let this = named_params! { ":repository": repository, ":number": number, ":found_in": picked };
    connection.execute(forget, this).map(drop)?;
    let picks = "SELECT sha FROM cherry_picks WHERE repository = :repository AND number = :number
                 ORDER BY sha";
    let mut picks = connection.prepare(picks)?;
    let this = named_params! { ":repository": repository, ":number": number };
    let shas = picks.query_map(this, |row| row.get::<_, CommitSha>("sha"))?;
    let merged = "SELECT number, merge_commit FROM pull_requests
                  WHERE repository = :repository AND merge_commit >= :sha
                  ORDER BY merge_commit LIMIT 1";
    let mut merged = connection.prepare(merged)?;
    for sha in shas {
        let sha = sha?;
        let mut found = merged.query(named_params! { ":repository": repository, ":sha": sha })?;
        let Some(row) = found.next()? else { continue };
        let (origin, whole): (u32, CommitSha) = (row.get("number")?, row.get("merge_commit")?);
        if whole.starts_with(&sha) {
            return save_origin(connection, (repository, number), picked, origin);
        }
    }
    Ok(())
}

/// The PRs whose cherry-pick lines name `merge_commit`, which PR `number` became, whole or cut short, and
/// those whose lines named PR `number` before.
fn pickers_of(
    connection: &Connection,
    repository: &RepositoryName,
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
    let query = "SELECT number FROM pr_origins
                 WHERE repository = :repository AND origin = :number AND found_in = :cherry_pick
                 UNION
                 SELECT pick.number AS number FROM json_each(:starts) AS start
                 CROSS JOIN cherry_picks AS pick
                     ON pick.repository = :repository AND pick.sha = start.value";
    let mut statement = connection.prepare(query)?;
    let values = named_params! {
        ":repository": repository,
        ":number": number,
        ":cherry_pick": FoundIn::CherryPick,
        ":starts": starts,
    };
    let pickers = statement.query_map(values, |row| row.get("number"))?;
    pickers.collect()
}

/// Fills the links of every PR the store holds, as the step that made the tables leaves them empty.
pub(super) fn fill(connection: &Connection) -> rusqlite::Result<()> {
    let mut statement = connection.prepare("SELECT DISTINCT repository FROM pull_requests")?;
    let names = statement.query_map([], |row| row.get::<_, String>("repository"))?;
    let names: Vec<String> = names.collect::<rusqlite::Result<_>>()?;
    // A name the store holds parsed once; one that no longer parses names no Repository to link.
    let repositories = names
        .iter()
        .filter_map(|name| RepositoryName::parse(name).ok());
    for repository in repositories {
        for facts in pull_requests::read_all(connection, &repository)? {
            rewrite(connection, &repository, &facts)?;
        }
    }
    Ok(())
}

/// The links of each PR of `numbers` that the store holds, under `conventions`, in number order.
pub(super) fn read(
    connection: &Connection,
    repository: &RepositoryName,
    conventions: Conventions,
    numbers: &Numbers,
) -> rusqlite::Result<Vec<PrLinks>> {
    let picked = Picked::of(conventions);
    let these: [(&str, &dyn ToSql); 2] = [(":repository", repository), (":numbers", numbers)];
    let fixes_query = format!(
        "SELECT fix.number AS number, fix.issue AS issue FROM pr_fixes AS fix
         WHERE fix.repository = :repository
             AND fix.number IN (SELECT value FROM json_each(:numbers)) AND {}
             AND EXISTS (SELECT 1 FROM issues AS issue
                         WHERE issue.repository = fix.repository AND issue.number = fix.issue)
         ORDER BY fix.number, fix.issue",
        Picked::FIXES
    );
    let asked = [&these[..], &picked.fixes()].concat();
    let mut fixes: BTreeMap<u32, Vec<u32>> =
        values_by_number(connection.prepare(&fixes_query)?, &asked, "issue")?;
    let origins_query = format!(
        "SELECT origin.number AS number, origin.origin AS origin FROM pr_origins AS origin
         WHERE origin.repository = :repository
             AND origin.number IN (SELECT value FROM json_each(:numbers)) AND {}
         ORDER BY origin.number, origin.found_in = :cherry_pick",
        Picked::ORIGINS
    );
    let asked = [&these[..], &picked.origins()].concat();
    // A title's marker sorts before a cherry-pick line, and comes first (ADR 0011).
    let mut origins: BTreeMap<u32, Vec<u32>> =
        values_by_number(connection.prepare(&origins_query)?, &asked, "origin")?;
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
    let picked = Picked::of(conventions);
    // Each listed Issue finds its PRs through `pr_fixes_by_issue`, which `CROSS JOIN` makes SQLite take.
    let query = format!(
        "SELECT fix.number AS number FROM json_each(:issues) AS listed
         CROSS JOIN pr_fixes AS fix ON fix.repository = :repository AND fix.issue = listed.value
         WHERE {}",
        Picked::FIXES
    );
    let these: [(&str, &dyn ToSql); 2] = [(":repository", repository), (":issues", issues)];
    let asked = [&these[..], &picked.fixes()].concat();
    let mut statement = connection.prepare(&query)?;
    let found = statement.query_map(asked.as_slice(), |row| row.get("number"))?;
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
    let picked = Picked::of(conventions);
    // Each listed PR finds what names it through `pr_origins_by_origin`, as `fixing` does.
    let query = format!(
        "SELECT origin.number AS number FROM json_each(:origins) AS listed
         CROSS JOIN pr_origins AS origin
             ON origin.repository = :repository AND origin.origin = listed.value
         WHERE {}",
        Picked::ORIGINS
    );
    let these: [(&str, &dyn ToSql); 2] = [(":repository", repository), (":origins", origins)];
    let asked = [&these[..], &picked.origins()].concat();
    let mut statement = connection.prepare(&query)?;
    let found = statement.query_map(asked.as_slice(), |row| row.get("number"))?;
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
