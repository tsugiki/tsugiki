use super::*;
use std::path::PathBuf;

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

/// The path of a database in a new temp folder, and that folder.
pub(super) fn temp_database(name: &str) -> (PathBuf, PathBuf) {
    let folder = crate::testing::temp_path(&format!("store-{name}"));
    fs::create_dir_all(&folder).unwrap();
    (folder.join("tsugiki.db"), folder)
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
