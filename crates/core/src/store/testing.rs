use super::*;
use crate::testing::TempPath;
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

/// The path of a database in a new temp folder, and that folder. Close the database before the folder drops.
pub(super) fn temp_database(name: &str) -> (PathBuf, TempPath) {
    let folder = crate::testing::temp_path(&format!("store-{name}"));
    fs::create_dir_all(&folder).unwrap();
    (folder.join("tsugiki.db"), folder)
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
        let (path, folder) = temp_database(name);
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
