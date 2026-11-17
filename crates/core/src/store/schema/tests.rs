use std::collections::BTreeSet;

use tsugiki_test_git::compare_or_update;

use super::*;
use crate::store::testing::migrated;

/// `TSUGIKI_UPDATE_SCHEMA=1` rewrites `docs/schema.md` instead of comparing; `just schema` does that.
#[test]
fn the_schema_file_draws_what_the_migrations_leave() {
    let fresh = markdown().unwrap();
    compare_or_update(
        "docs/schema.md",
        &fresh,
        "TSUGIKI_UPDATE_SCHEMA",
        "just schema",
    );
}

#[test]
fn each_table_the_migrations_leave_is_on_the_cache_or_the_state_side() {
    let made: BTreeSet<String> = tables(&migrated()).unwrap().into_iter().collect();
    let listed: BTreeSet<String> = TABLES
        .iter()
        .map(|(table, _)| (*table).to_owned())
        .collect();

    let unlisted: Vec<&String> = made.difference(&listed).collect();
    let gone: Vec<&String> = listed.difference(&made).collect();
    assert!(
        unlisted.is_empty(),
        "add each of {unlisted:?} to TABLES in store/schema.rs, on the cache or the state side"
    );
    assert!(
        gone.is_empty(),
        "TABLES in store/schema.rs lists {gone:?}, which no migration makes"
    );
}

#[test]
fn a_diagram_gives_each_column_its_type_and_keys_and_a_line_for_each_foreign_key() {
    let connection = Connection::open_in_memory().unwrap();
    // A key column may hold null in a table with rowids, unless it says NOT NULL, and never in one without.
    connection
        .execute_batch(
            "CREATE TABLE posts (
                 repository TEXT NOT NULL,
                 number INTEGER NOT NULL,
                 body,
                 PRIMARY KEY (repository, number)
             );
             CREATE TABLE drafts (title TEXT PRIMARY KEY);
             CREATE TABLE replies (
                 repository TEXT,
                 number INTEGER,
                 position INTEGER,
                 quoted INTEGER,
                 PRIMARY KEY (repository, number, position),
                 FOREIGN KEY (repository, quoted) REFERENCES posts (repository, number),
                 FOREIGN KEY (repository, number) REFERENCES posts (repository, number)
             ) WITHOUT ROWID;",
        )
        .unwrap();

    let drawn = mermaid(&connection, &["posts", "drafts", "replies"]).unwrap();

    assert_eq!(
        drawn,
        "erDiagram
    posts {
        TEXT repository PK
        INTEGER number PK
        ANY body \"nullable\"
    }
    drafts {
        TEXT title PK \"nullable\"
    }
    replies {
        TEXT repository PK, FK
        INTEGER number PK, FK
        INTEGER position PK
        INTEGER quoted FK \"nullable\"
    }
    replies }o--|| posts : \"repository, number\"
    replies }o--o| posts : \"repository, quoted\"
"
    );
}
