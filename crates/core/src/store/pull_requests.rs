//! The PRs Tsugiki keeps, as facts (ADR 0010). What a fact means for a screen is the core's to work out when
//! the screen asks.

use std::collections::BTreeMap;

use rusqlite::types::FromSql;
use rusqlite::{OptionalExtension, Statement, ToSql, named_params};
use time::OffsetDateTime;

use super::activity::StateMove;
use super::{Connection, TitledItem, TitledKind};
use crate::facts::CommitSha;
use crate::facts::activity::{ItemActivity, ItemKey};
use crate::facts::labels::Label;
use crate::facts::pull_requests::{LatestReview, OffDefault, PrState, PullRequestFacts};
use crate::login::Login;
use crate::repository::RepositoryName;

use super::columns::{Items, UnixTime};
use super::{activity, pr_links};

pub(super) const SCHEMA: &str = "
    CREATE TABLE pull_requests (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        title TEXT NOT NULL,
        state TEXT NOT NULL,
        is_draft INTEGER NOT NULL,
        author TEXT,
        target_branch TEXT NOT NULL,
        ci TEXT,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY (repository, number)
    ) WITHOUT ROWID;
    CREATE INDEX pull_requests_by_update ON pull_requests (repository, state, updated_at);
    CREATE TABLE review_requests (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        login TEXT NOT NULL COLLATE NOCASE,
        PRIMARY KEY (repository, number, login),
        FOREIGN KEY (repository, number) REFERENCES pull_requests (repository, number)
    ) WITHOUT ROWID;
    CREATE TABLE latest_reviews (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        login TEXT NOT NULL COLLATE NOCASE,
        state TEXT NOT NULL,
        PRIMARY KEY (repository, number, login),
        FOREIGN KEY (repository, number) REFERENCES pull_requests (repository, number)
    ) WITHOUT ROWID;
    CREATE TABLE assignees (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        login TEXT NOT NULL COLLATE NOCASE,
        PRIMARY KEY (repository, number, login),
        FOREIGN KEY (repository, number) REFERENCES pull_requests (repository, number)
    ) WITHOUT ROWID;
";

/// The facts the link rules read (ADR 0011). A PR stored before this has neither until the next sync fetches
/// it.
pub(super) const LINK_FACTS: &str = "
    ALTER TABLE pull_requests ADD COLUMN off_default_description TEXT;
    CREATE TABLE closing_references (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        issue INTEGER NOT NULL,
        PRIMARY KEY (repository, number, issue),
        FOREIGN KEY (repository, number) REFERENCES pull_requests (repository, number)
    ) WITHOUT ROWID;
";

/// The commit each PR's branch is at, which the Repository's mirror fetches (#157). A PR stored before this
/// has none until the next sync fetches it.
pub(super) const HEADS: &str = "ALTER TABLE pull_requests ADD COLUMN head TEXT;";

/// GitHub's `updatedAt` at the read whose head the row keeps, a PR list's or a Conversation's (#905). A
/// read whose `updatedAt` is later replaces the head. A head kept before this counts from the PR's last list
/// read. A head a Conversation filled in (#782) may come from a later read, which the store kept no time of,
/// so a lagging list read between the two can still replace it once.
pub(super) const HEAD_AT: &str = "
    ALTER TABLE pull_requests ADD COLUMN head_at INTEGER;
    UPDATE pull_requests SET head_at = updated_at WHERE head IS NOT NULL;
";

/// The commit of the Target branch GitHub compares each PR with (#212). A merged PR stored before this has
/// none, and stays frozen without it.
pub(super) const BASES: &str = "ALTER TABLE pull_requests ADD COLUMN base TEXT;";

/// The labels of each PR, which a search of the cache matches (#463). A PR stored before this has none
/// until a sync reads it again, which a merged or closed PR waits for until it changes.
pub(super) const LABELS: &str = "
    CREATE TABLE pull_request_labels (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        name TEXT NOT NULL COLLATE NOCASE,
        PRIMARY KEY (repository, number, name),
        FOREIGN KEY (repository, number) REFERENCES pull_requests (repository, number)
    ) WITHOUT ROWID;
";

/// The facts the Backport rules read (ADR 0011). A PR stored before this has none, so it isn't a Bot
/// backport until the next sync fetches it. A merged one isn't frozen until then either, because it has no
/// merge commit.
pub(super) const BACKPORT_FACTS: &str = "
    ALTER TABLE pull_requests ADD COLUMN merge_commit TEXT;
    CREATE TABLE cherry_picks (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        sha TEXT NOT NULL,
        PRIMARY KEY (repository, number, sha),
        FOREIGN KEY (repository, number) REFERENCES pull_requests (repository, number)
    ) WITHOUT ROWID;
    CREATE TABLE committers (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        position INTEGER NOT NULL,
        login TEXT,
        PRIMARY KEY (repository, number, position),
        FOREIGN KEY (repository, number) REFERENCES pull_requests (repository, number)
    ) WITHOUT ROWID;
";

/// Stores `pull_requests` in one transaction, each replacing what Tsugiki knew about it (ADR 0010).
pub(super) fn save(
    connection: &mut Connection,
    repository: &RepositoryName,
    pull_requests: &[PullRequestFacts],
    events: &[ItemActivity],
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    save_in(&transaction, repository, pull_requests, events)?;
    transaction.commit()
}

/// Does what `save` does inside a transaction the caller holds, which can save more beside it.
pub(super) fn save_in(
    transaction: &Connection,
    repository: &RepositoryName,
    pull_requests: &[PullRequestFacts],
    events: &[ItemActivity],
) -> rusqlite::Result<()> {
    // Before the PRs, because a commit counts differently on a PR the store holds.
    activity::save(transaction, repository, events)?;
    let mut keeper = pr_links::Keeper::new(transaction, repository)?;
    for pull_request in pull_requests {
        replace(transaction, (repository, &mut keeper), pull_request)?;
    }
    Ok(())
}

/// Replaces what the store knew about the PR of `facts`, and the links `keeper` keeps for it, unless the
/// store holds a newer read of it. A replica of GitHub's that lags can serve an older read.
fn replace(
    connection: &Connection,
    (repository, keeper): (&RepositoryName, &mut pr_links::Keeper<'_>),
    facts: &PullRequestFacts,
) -> rusqlite::Result<()> {
    let number = facts.number;
    let held: Option<PrState> = connection
        .query_row(
            "SELECT state FROM pull_requests WHERE repository = :repository AND number = :number",
            named_params! { ":repository": repository, ":number": number },
            |row| row.get("state"),
        )
        .optional()?;
    if !save_pull_request(connection, repository, facts)? {
        return Ok(());
    }
    let is_open = |state: PrState| Some(state == PrState::Open);
    let moved = StateMove::of(held.and_then(is_open), is_open(facts.state));
    let key = ItemKey::PullRequest { number };
    activity::follow_state(connection, repository, key, moved)?;
    if let Some(head) = &facts.head {
        let _moved = keep_head(connection, repository, number, (head, facts.updated_at))?;
    }
    forget_review_requests(connection, repository, number)?;
    forget_latest_reviews(connection, repository, number)?;
    forget_assignees(connection, repository, number)?;
    forget_labels(connection, repository, number)?;
    forget_closing_references(connection, repository, number)?;
    forget_cherry_picks(connection, repository, number)?;
    forget_committers(connection, repository, number)?;
    if let Some(off_default) = &facts.off_default {
        save_off_default(connection, repository, number, off_default)?;
    }
    for login in &facts.review_requests {
        save_review_request(connection, repository, number, login)?;
    }
    for review in &facts.latest_reviews {
        save_latest_review(connection, repository, number, review)?;
    }
    for login in &facts.assignees {
        save_assignee(connection, repository, number, login)?;
    }
    for label in &facts.labels {
        save_label(connection, repository, number, label)?;
    }
    for issue in &facts.closing_issues {
        save_closing_reference(connection, repository, number, *issue)?;
    }
    // After the cherry-picks, which it reads.
    keeper.rewrite(facts)?;
    if held.is_none() {
        // After the links, which it reads.
        activity::note_opening(connection, repository, number)?;
    }
    Ok(())
}

/// Writes the facts of `facts` that `pull_requests` holds beside the head, unless the row holds a read with
/// a later `updatedAt`. A read as old as the row replaces it, as a probe reads CI while `updatedAt` stays.
/// Says whether the row took the read.
fn save_pull_request(
    connection: &Connection,
    repository: &RepositoryName,
    facts: &PullRequestFacts,
) -> rusqlite::Result<bool> {
    let saved = connection.execute(
        "INSERT INTO pull_requests
                 (repository, number, title, state, is_draft, author, target_branch, ci, updated_at,
                  off_default_description, merge_commit, base, ci_finished_at)
             VALUES (:repository, :number, :title, :state, :is_draft, :author, :target_branch, :ci,
                     :updated_at, :off_default_description, :merge_commit, :base, CASE
                         WHEN :ci IN ('SUCCESS', 'FAILURE', 'ERROR')
                             AND EXISTS (SELECT 1 FROM first_syncs WHERE repository = :repository)
                         THEN :now END)
             ON CONFLICT (repository, number) DO UPDATE SET
                 title = excluded.title,
                 state = excluded.state,
                 is_draft = excluded.is_draft,
                 author = excluded.author,
                 target_branch = excluded.target_branch,
                 ci = excluded.ci,
                 ci_finished_at = CASE
                     WHEN excluded.ci IS NOT pull_requests.ci
                         AND excluded.ci IN ('SUCCESS', 'FAILURE', 'ERROR')
                     THEN :now ELSE pull_requests.ci_finished_at END,
                 checks_read_at = CASE
                     WHEN excluded.updated_at > pull_requests.updated_at THEN NULL
                     ELSE pull_requests.checks_read_at END,
                 updated_at = excluded.updated_at,
                 off_default_description = excluded.off_default_description,
                 merge_commit = excluded.merge_commit,
                 base = excluded.base
             WHERE excluded.updated_at >= pull_requests.updated_at",
        named_params! {
            ":repository": repository,
            ":number": facts.number,
            ":title": facts.title,
            ":state": facts.state,
            ":is_draft": facts.is_draft,
            ":author": facts.author,
            ":target_branch": facts.target_branch,
            ":ci": facts.ci,
            ":updated_at": facts.updated_at.unix_timestamp(),
            ":off_default_description": facts.off_default.as_ref().map(|off| &off.description),
            ":merge_commit": facts.merge_commit,
            ":base": facts.base,
            ":now": OffsetDateTime::now_utc().unix_timestamp(),
        },
    );
    saved.map(|written| written > 0)
}

/// Keeps `head` as PR `number`'s head when the read that names it, whose `updatedAt` is `updated_at`, is
/// later than the read whose head the row keeps, or the row keeps none. A tie leaves the head. Both reads
/// that name a head, the PR list and the Conversation, write it here, so the store holds the newer one's.
/// Says whether the head moved to another commit.
pub(super) fn keep_head(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
    (head, updated_at): (&CommitSha, OffsetDateTime),
) -> rusqlite::Result<bool> {
    let asked = named_params! {
        ":repository": repository,
        ":number": number,
        ":head": head,
        ":updated_at": updated_at.unix_timestamp(),
    };
    let moved = connection.execute(
        "UPDATE pull_requests SET head = :head, head_at = :updated_at
         WHERE repository = :repository AND number = :number
           AND (head_at IS NULL OR head_at < :updated_at) AND head IS NOT :head",
        asked,
    );
    let moved = moved? > 0;
    // The same head, named by a newer read, dates from that read.
    let dated = connection.execute(
        "UPDATE pull_requests SET head_at = :updated_at
         WHERE repository = :repository AND number = :number AND head = :head
           AND head_at < :updated_at",
        asked,
    );
    dated.map(|_dated| moved)
}

fn forget_review_requests(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "DELETE FROM review_requests WHERE repository = :repository AND number = :number",
            named_params! { ":repository": repository, ":number": number },
        )
        .map(drop)
}

fn forget_latest_reviews(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "DELETE FROM latest_reviews WHERE repository = :repository AND number = :number",
            named_params! { ":repository": repository, ":number": number },
        )
        .map(drop)
}

fn forget_assignees(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "DELETE FROM assignees WHERE repository = :repository AND number = :number",
            named_params! { ":repository": repository, ":number": number },
        )
        .map(drop)
}

fn forget_labels(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "DELETE FROM pull_request_labels WHERE repository = :repository AND number = :number",
            named_params! { ":repository": repository, ":number": number },
        )
        .map(drop)
}

fn save_label(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
    label: &Label,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT OR IGNORE INTO pull_request_labels (repository, number, name)
             VALUES (:repository, :number, :name)",
            named_params! { ":repository": repository, ":number": number, ":name": label },
        )
        .map(drop)
}

/// The state a read of the open PRs binds as `:open`.
const OPEN: PrState = PrState::Open;

/// What follows `FROM` to read the rows of `table` that belong to the PRs `items` takes, as `item`, up to
/// the end of a `WHERE`.
fn rows_of(items: Items<'_>, table: &str) -> String {
    let condition = items.condition("item.number");
    format!("{table} AS item WHERE item.repository = :repository {condition}")
}

/// The parameters `rows_of` names.
fn params_of<'p>(
    items: Items<'p>,
    repository: &'p RepositoryName,
) -> Vec<(&'p str, &'p dyn ToSql)> {
    items.with(vec![(":repository", repository)])
}

fn forget_closing_references(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "DELETE FROM closing_references WHERE repository = :repository AND number = :number",
            named_params! { ":repository": repository, ":number": number },
        )
        .map(drop)
}

fn forget_cherry_picks(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "DELETE FROM cherry_picks WHERE repository = :repository AND number = :number",
            named_params! { ":repository": repository, ":number": number },
        )
        .map(drop)
}

fn forget_committers(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "DELETE FROM committers WHERE repository = :repository AND number = :number",
            named_params! { ":repository": repository, ":number": number },
        )
        .map(drop)
}

fn save_off_default(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
    off_default: &OffDefault,
) -> rusqlite::Result<()> {
    for sha in &off_default.cherry_picked_from {
        save_cherry_pick(connection, repository, number, sha)?;
    }
    for (position, login) in (0_u32..).zip(&off_default.committers) {
        let query = "INSERT INTO committers (repository, number, position, login)
                     VALUES (:repository, :number, :position, :login)";
        let values = named_params! {
            ":repository": repository,
            ":number": number,
            ":position": position,
            ":login": login,
        };
        connection.execute(query, values).map(drop)?;
    }
    Ok(())
}

fn save_cherry_pick(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
    sha: &CommitSha,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT OR IGNORE INTO cherry_picks (repository, number, sha)
             VALUES (:repository, :number, :sha)",
            named_params! { ":repository": repository, ":number": number, ":sha": sha },
        )
        .map(drop)
}

fn save_review_request(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
    login: &Login,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT OR IGNORE INTO review_requests (repository, number, login)
             VALUES (:repository, :number, :login)",
            named_params! { ":repository": repository, ":number": number, ":login": login },
        )
        .map(drop)
}

fn save_latest_review(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
    review: &LatestReview,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT OR IGNORE INTO latest_reviews (repository, number, login, state)
             VALUES (:repository, :number, :login, :state)",
            named_params! {
                ":repository": repository,
                ":number": number,
                ":login": review.author,
                ":state": review.state,
            },
        )
        .map(drop)
}

fn save_assignee(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
    login: &Login,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT OR IGNORE INTO assignees (repository, number, login)
             VALUES (:repository, :number, :login)",
            named_params! { ":repository": repository, ":number": number, ":login": login },
        )
        .map(drop)
}

fn save_closing_reference(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
    issue: u32,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT OR IGNORE INTO closing_references (repository, number, issue)
             VALUES (:repository, :number, :issue)",
            named_params! { ":repository": repository, ":number": number, ":issue": issue },
        )
        .map(drop)
}

/// Each open PR's Review requests. A merged or closed PR asks nothing of anyone, so its requests stay stored
/// and go unread.
fn review_requests(
    connection: &Connection,
    repository: &RepositoryName,
    items: Items<'_>,
) -> rusqlite::Result<BTreeMap<u32, Vec<Login>>> {
    let rows = rows_of(items, "review_requests");
    let mut asked = params_of(items, repository);
    asked.push((":open", &OPEN));
    let query = format!(
        "SELECT item.number AS number, item.login AS login FROM {rows}
             AND EXISTS (SELECT 1 FROM pull_requests AS asking
                         WHERE asking.repository = item.repository AND asking.number = item.number
                             AND asking.state = :open)
         ORDER BY item.number, item.login"
    );
    values_by_number(&mut connection.prepare(&query)?, &asked, "login")
}

/// The `column` of each row of `table` that belongs to a PR `items` takes, by number, each PR's in the
/// column's order.
fn values_of<T: FromSql>(
    connection: &Connection,
    (repository, items): (&RepositoryName, Items<'_>),
    (table, column): (&str, &str),
) -> rusqlite::Result<BTreeMap<u32, Vec<T>>> {
    let rows = rows_of(items, table);
    let query = format!(
        "SELECT item.number AS number, item.{column} AS {column} FROM {rows}
         ORDER BY item.number, item.{column}"
    );
    let asked = params_of(items, repository);
    values_by_number(&mut connection.prepare(&query)?, &asked, column)
}

/// Each item's values in the `value` column of what `statement` answers, by its `number` column. Each
/// column is found by name once, because a search on every row costs more than reading it.
pub(super) fn values_by_number<T: FromSql>(
    statement: &mut Statement<'_>,
    params: &[(&str, &dyn ToSql)],
    value: &str,
) -> rusqlite::Result<BTreeMap<u32, Vec<T>>> {
    let number = statement.column_index("number")?;
    let value = statement.column_index(value)?;
    let mut found: BTreeMap<u32, Vec<T>> = BTreeMap::new();
    let mut rows = statement.query(params)?;
    while let Some(row) = rows.next()? {
        let item = row.get(value)?;
        found.entry(row.get(number)?).or_default().push(item);
    }
    Ok(found)
}

fn latest_reviews(
    connection: &Connection,
    repository: &RepositoryName,
    items: Items<'_>,
) -> rusqlite::Result<BTreeMap<u32, Vec<LatestReview>>> {
    let mut found: BTreeMap<u32, Vec<LatestReview>> = BTreeMap::new();
    let rows = rows_of(items, "latest_reviews");
    let query = format!(
        "SELECT item.number AS number, item.login AS login, item.state AS state FROM {rows}
         ORDER BY item.number, item.login"
    );
    let mut statement = connection.prepare(&query)?;
    let number = statement.column_index("number")?;
    let login = statement.column_index("login")?;
    let state = statement.column_index("state")?;
    let asked = params_of(items, repository);
    let mut rows = statement.query(asked.as_slice())?;
    while let Some(row) = rows.next()? {
        let review = LatestReview {
            author: row.get(login)?,
            state: row.get(state)?,
        };
        found.entry(row.get(number)?).or_default().push(review);
    }
    Ok(found)
}

/// Each PR's committers, in the order of its commits.
fn committers(
    connection: &Connection,
    repository: &RepositoryName,
    items: Items<'_>,
) -> rusqlite::Result<BTreeMap<u32, Vec<Option<Login>>>> {
    let rows = rows_of(items, "committers");
    let query = format!(
        "SELECT item.number AS number, item.login AS login FROM {rows}
         ORDER BY item.number, item.position"
    );
    let asked = params_of(items, repository);
    values_by_number(&mut connection.prepare(&query)?, &asked, "login")
}

/// The position of each of a PR's columns in the answer to `read_all`, found once, as `values_by_number`
/// does.
struct PullRequestColumns {
    number: usize,
    title: usize,
    state: usize,
    is_draft: usize,
    author: usize,
    target_branch: usize,
    ci: usize,
    updated_at: usize,
    off_default_description: usize,
    merge_commit: usize,
    base: usize,
    head: usize,
}

impl PullRequestColumns {
    fn of(statement: &Statement<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            number: statement.column_index("number")?,
            title: statement.column_index("title")?,
            state: statement.column_index("state")?,
            is_draft: statement.column_index("is_draft")?,
            author: statement.column_index("author")?,
            target_branch: statement.column_index("target_branch")?,
            ci: statement.column_index("ci")?,
            updated_at: statement.column_index("updated_at")?,
            off_default_description: statement.column_index("off_default_description")?,
            merge_commit: statement.column_index("merge_commit")?,
            base: statement.column_index("base")?,
            head: statement.column_index("head")?,
        })
    }
}

/// The number, title, state and last update of each PR of `repository`, which quick open searches.
pub(super) fn read_titled(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<Vec<TitledItem>> {
    let query = "SELECT number, title, updated_at, state, is_draft FROM pull_requests
                 WHERE repository = :repository";
    let mut statement = connection.prepare(query)?;
    let titled = statement.query_map(named_params! { ":repository": repository }, |row| {
        let UnixTime(updated_at) = row.get("updated_at")?;
        Ok(TitledItem {
            number: row.get("number")?,
            title: row.get("title")?,
            updated_at,
            kind: TitledKind::PullRequest {
                state: row.get("state")?,
                is_draft: row.get("is_draft")?,
            },
        })
    });
    titled?.collect()
}

/// Every PR of `repository`, whatever its state, the latest update first.
pub(super) fn read_all(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<Vec<PullRequestFacts>> {
    read(connection, repository, Items::All)
}

/// The PRs of `repository` that `items` names, the latest update first.
pub(super) fn read(
    connection: &Connection,
    repository: &RepositoryName,
    items: Items<'_>,
) -> rusqlite::Result<Vec<PullRequestFacts>> {
    let of = (repository, items);
    let mut requests = review_requests(connection, repository, items)?;
    let mut reviews = latest_reviews(connection, repository, items)?;
    let mut assigned: BTreeMap<u32, Vec<Login>> =
        values_of(connection, of, ("assignees", "login"))?;
    // In name order, ignoring case, as `PullRequestFacts` holds them.
    let mut labelled: BTreeMap<u32, Vec<Label>> =
        values_of(connection, of, ("pull_request_labels", "name"))?;
    let mut closing: BTreeMap<u32, Vec<u32>> =
        values_of(connection, of, ("closing_references", "issue"))?;
    let mut picks: BTreeMap<u32, Vec<CommitSha>> =
        values_of(connection, of, ("cherry_picks", "sha"))?;
    let mut committed = committers(connection, repository, items)?;
    let condition = items.condition("number");
    // The `+` keeps SQLite from walking `pull_requests_by_time` over every PR of the Repository to save a
    // sort, when the listed numbers would each find their PR by its key.
    let query = format!(
        "SELECT number, title, state, is_draft, author, target_branch, ci, updated_at,
                off_default_description, merge_commit, base, head
         FROM pull_requests
         WHERE repository = :repository {condition}
         ORDER BY +updated_at DESC, number DESC"
    );
    let mut statement = connection.prepare(&query)?;
    let column = PullRequestColumns::of(&statement)?;
    let asked = params_of(items, repository);
    let mut rows = statement.query(asked.as_slice())?;
    let mut found = Vec::new();
    while let Some(row) = rows.next()? {
        let number: u32 = row.get(column.number)?;
        let UnixTime(updated_at) = row.get(column.updated_at)?;
        let description: Option<String> = row.get(column.off_default_description)?;
        found.push(PullRequestFacts {
            number,
            title: row.get(column.title)?,
            state: row.get(column.state)?,
            is_draft: row.get(column.is_draft)?,
            author: row.get(column.author)?,
            target_branch: row.get(column.target_branch)?,
            ci: row.get(column.ci)?,
            updated_at,
            review_requests: requests.remove(&number).unwrap_or_default(),
            latest_reviews: reviews.remove(&number).unwrap_or_default(),
            assignees: assigned.remove(&number).unwrap_or_default(),
            labels: labelled.remove(&number).unwrap_or_default(),
            closing_issues: closing.remove(&number).unwrap_or_default(),
            merge_commit: row.get(column.merge_commit)?,
            base: row.get(column.base)?,
            head: row.get(column.head)?,
            off_default: description.map(|description| OffDefault {
                description,
                cherry_picked_from: picks.remove(&number).unwrap_or_default(),
                committers: committed.remove(&number).unwrap_or_default(),
            }),
        });
    }
    Ok(found)
}

#[cfg(test)]
mod tests;
