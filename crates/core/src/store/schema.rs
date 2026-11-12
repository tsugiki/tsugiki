//! The database schema as Mermaid `erDiagram`s, read from the tables `MIGRATIONS` leave, for
//! `docs/schema.md` (#323). GitHub draws the diagrams in the file, and a test fails when the file and the
//! migrations disagree, so the drawing can't go stale.

use std::collections::BTreeMap;

use rusqlite::named_params;

use super::Connection;
use super::testing::migrated;

/// Which side of ADR 0010's split a table is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    /// What GitHub said about the Issues and PRs, and their timelines. A timeline keeps events GitHub's
    /// answer no longer lists, so not all of it can be read again.
    Cache,
    /// The user's own state, and the sync's: where it stands and what each Filter's search found.
    State,
}

/// The diagrams the file draws, a few tables each, since Mermaid's layout gets cramped at 25.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Diagram {
    PullRequests,
    Issues,
    Filters,
    UserAndSync,
}

impl Diagram {
    const ALL: [Self; 4] = [
        Self::PullRequests,
        Self::Issues,
        Self::Filters,
        Self::UserAndSync,
    ];

    fn side(self) -> Side {
        match self {
            Self::PullRequests | Self::Issues => Side::Cache,
            Self::Filters | Self::UserAndSync => Side::State,
        }
    }

    fn heading(self) -> &'static str {
        match self {
            Self::PullRequests => "PRs",
            Self::Issues => "Issues, their timelines and the viewer",
            Self::Filters => "Filters",
            Self::UserAndSync => "Last seen, Marked unseen and the sync's place",
        }
    }
}

/// Every table, with the diagram it goes in. A table missing here fails a test, so each new one gets a side.
const TABLES: &[(&str, Diagram)] = &[
    ("pull_requests", Diagram::PullRequests),
    ("review_requests", Diagram::PullRequests),
    ("latest_reviews", Diagram::PullRequests),
    ("assignees", Diagram::PullRequests),
    ("pull_request_labels", Diagram::PullRequests),
    ("closing_references", Diagram::PullRequests),
    ("cherry_picks", Diagram::PullRequests),
    ("committers", Diagram::PullRequests),
    ("failed_checks", Diagram::PullRequests),
    ("pr_reviews", Diagram::PullRequests),
    ("review_threads", Diagram::PullRequests),
    ("review_thread_comments", Diagram::PullRequests),
    ("viewer", Diagram::Issues),
    ("issues", Diagram::Issues),
    ("issue_assignees", Diagram::Issues),
    ("issue_labels", Diagram::Issues),
    ("issue_content", Diagram::Issues),
    ("issue_comments", Diagram::Issues),
    ("activity", Diagram::Issues),
    ("filter_matches", Diagram::Filters),
    ("filter_problems", Diagram::Filters),
    ("filter_searches", Diagram::Filters),
    ("last_seen", Diagram::UserAndSync),
    ("marked_unseen", Diagram::UserAndSync),
    ("first_syncs", Diagram::UserAndSync),
    ("sync_state", Diagram::UserAndSync),
    ("first_sync_place", Diagram::UserAndSync),
    ("read_ahead", Diagram::UserAndSync),
];

/// `docs/schema.md` as the migrations leave the schema: each diagram's tables under its side.
fn markdown() -> rusqlite::Result<String> {
    let connection = migrated();
    let mut text = String::from(HEADER);
    for (side, heading) in [
        (Side::Cache, "## The cache: what GitHub says"),
        (Side::State, "## State: the user's and the sync's"),
    ] {
        text.push_str(&format!("\n{heading}\n"));
        for diagram in Diagram::ALL
            .into_iter()
            .filter(|diagram| diagram.side() == side)
        {
            let tables: Vec<&str> = TABLES
                .iter()
                .filter(|(_, on)| *on == diagram)
                .map(|(table, _)| *table)
                .collect();
            let drawn = mermaid(&connection, &tables)?;
            let heading = diagram.heading();
            text.push_str(&format!("\n### {heading}\n\n```mermaid\n{drawn}```\n"));
        }
    }
    Ok(text)
}

const HEADER: &str = "\
# The database schema

<!-- Written by `just schema` from crates/core/src/store/migrate.rs. Don't edit it by hand. -->

The tables of `tsugiki.db` as its migrations leave them, cache apart from state
([ADR 0010](adr/0010-one-sqlite-database-and-per-repository-mirrors.md)). `PK` marks a table's primary key
and `FK` a column that points at another table's row, and a column that may hold null says so. A line joins
the table that points to the one it points at, and its label names the columns.
";

/// The names of the tables of the database `connection` opens, SQLite's own left out.
fn tables(connection: &Connection) -> rusqlite::Result<Vec<String>> {
    let query = "SELECT name FROM sqlite_master
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
                 ORDER BY name";
    let mut statement = connection.prepare(query)?;
    let names = statement.query_map([], |row| row.get("name"));
    names?.collect()
}

/// A Mermaid `erDiagram` of `tables`: each one's columns with their types and keys, then a line for each
/// foreign key. A key's table that isn't one of `tables` still gets drawn, as an empty box.
fn mermaid(connection: &Connection, tables: &[&str]) -> rusqlite::Result<String> {
    let mut text = String::from("erDiagram\n");
    let mut lines = Vec::new();
    for table in tables {
        let columns = columns(connection, table)?;
        let keys = foreign_keys(connection, table, &columns)?;
        let points = |name: &str| {
            keys.iter()
                .any(|key| key.columns.iter().any(|column| column == name))
        };
        text.push_str(&format!("    {table} {{\n"));
        for column in &columns {
            let marks = match (column.key, points(&column.name)) {
                (Key::Primary, true) => " PK, FK",
                (Key::Primary, false) => " PK",
                (Key::Other, true) => " FK",
                (Key::Other, false) => "",
            };
            let nullable = match column.nulls {
                Nulls::Allowed => " \"nullable\"",
                Nulls::NotNull => "",
            };
            let Column { kind, name, .. } = column;
            text.push_str(&format!("        {kind} {name}{marks}{nullable}\n"));
        }
        text.push_str("    }\n");
        for key in keys {
            // A row whose key columns may be null points at nothing, so the parent's end may be zero.
            let parent_end = if key.optional { "o|" } else { "||" };
            let joined = key.columns.join(", ");
            lines.push(format!(
                "    {table} }}o--{parent_end} {} : \"{joined}\"\n",
                key.parent
            ));
        }
    }
    text.extend(lines);
    Ok(text)
}

/// A column as the diagram shows it.
struct Column {
    name: String,
    /// The declared type, as one word Mermaid takes: `ANY` for a column declared without one.
    kind: String,
    key: Key,
    nulls: Nulls,
}

/// Whether a column is part of its table's primary key.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Key {
    Primary,
    Other,
}

/// Whether a column may hold null.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Nulls {
    NotNull,
    Allowed,
}

fn columns(connection: &Connection, table: &str) -> rusqlite::Result<Vec<Column>> {
    let listed = "SELECT wr FROM pragma_table_list WHERE name = :table";
    let params = named_params! { ":table": table };
    let without_rowid: bool = connection.query_row(listed, params, |row| row.get("wr"))?;
    let query = "SELECT name, type, pk, \"notnull\" FROM pragma_table_info(:table) ORDER BY cid";
    let mut statement = connection.prepare(query)?;
    let found = statement.query_map(named_params! { ":table": table }, |row| {
        let declared: String = row.get("type")?;
        let not_null: bool = row.get("notnull")?;
        let key = if row.get::<_, u32>("pk")? > 0 {
            Key::Primary
        } else {
            Key::Other
        };
        Ok((row.get("name")?, declared, key, not_null))
    });
    let found: Vec<(String, String, Key, bool)> = found?.collect::<rusqlite::Result<_>>()?;
    let key_columns = found
        .iter()
        .filter(|(_, _, key, _)| *key == Key::Primary)
        .count();
    Ok(found
        .into_iter()
        .map(|(name, declared, key, not_null)| {
            // A key column may hold null in a table with rowids, unless it is that table's one INTEGER key,
            // which is the rowid itself.
            let rowid = key_columns == 1 && declared.eq_ignore_ascii_case("INTEGER");
            let nulls = match key {
                Key::Primary if without_rowid || rowid => Nulls::NotNull,
                Key::Primary | Key::Other if not_null => Nulls::NotNull,
                Key::Primary | Key::Other => Nulls::Allowed,
            };
            let kind = if declared.is_empty() {
                "ANY".to_owned()
            } else {
                declared.replace(|char: char| !char.is_ascii_alphanumeric(), "_")
            };
            Column {
                name,
                kind,
                key,
                nulls,
            }
        })
        .collect())
}

/// A foreign key of a table: the columns that point, and the table they point at.
struct ForeignKey {
    parent: String,
    columns: Vec<String>,
    /// Whether one of `columns` may be null.
    optional: bool,
}

/// The foreign keys of `table`, whose columns are `columns`.
fn foreign_keys(
    connection: &Connection,
    table: &str,
    columns: &[Column],
) -> rusqlite::Result<Vec<ForeignKey>> {
    let allows_null = |name: &str| {
        columns
            .iter()
            .any(|column| column.name == name && column.nulls == Nulls::Allowed)
    };
    let query =
        "SELECT id, \"table\" AS parent, \"from\" AS child FROM pragma_foreign_key_list(:table)
                 ORDER BY id, seq";
    let mut statement = connection.prepare(query)?;
    let mut rows = statement.query(named_params! { ":table": table })?;
    let mut keys: BTreeMap<u32, ForeignKey> = BTreeMap::new();
    while let Some(row) = rows.next()? {
        let column: String = row.get("child")?;
        let parent: String = row.get("parent")?;
        let key = keys.entry(row.get("id")?).or_insert_with(|| ForeignKey {
            parent,
            columns: Vec::new(),
            optional: false,
        });
        key.optional |= allows_null(&column);
        key.columns.push(column);
    }
    // In an order that SQLite's numbering of the keys doesn't change.
    let mut keys: Vec<ForeignKey> = keys.into_values().collect();
    keys.sort_by(|one, other| (&one.parent, &one.columns).cmp(&(&other.parent, &other.columns)));
    Ok(keys)
}

#[cfg(test)]
mod tests;
