use super::super::file::{Contents, contents};
use super::super::open_with;
use super::super::testing::{
    V1, V2, V3, V3_BROKEN, backups, has_table, temp_database, user_version,
};
use super::*;

#[test]
fn an_older_database_moves_forward_and_leaves_a_backup_of_how_it_was() {
    let (path, folder) = temp_database("older");
    open_with(&path, &[V1]).unwrap().close();

    open_with(&path, &[V1, V2]).unwrap().close();

    assert_eq!(user_version(&path), 2);
    assert!(has_table(&path, "tags"));
    assert_eq!(backups(&folder), ["tsugiki.db.v1.bak"]);
    let backup = folder.join("tsugiki.db.v1.bak");
    assert_eq!(user_version(&backup), 1);
    assert!(has_table(&backup, "notes"));
    assert!(!has_table(&backup, "tags"));
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_current_database_leaves_no_backup() {
    let (path, folder) = temp_database("current");
    open_with(&path, &[V1]).unwrap().close();
    open_with(&path, &[V1]).unwrap().close();
    assert_eq!(backups(&folder), [""; 0]);
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_failed_migration_changes_nothing_and_can_be_tried_again() {
    let (path, folder) = temp_database("failed");
    open_with(&path, &[V1]).unwrap().close();

    let problem = open_with(&path, &[V1, V2, V3_BROKEN]).err().unwrap();

    assert_eq!(problem.kind, ErrorKind::Internal);
    assert!(problem.message.contains("update"), "{}", problem.message);
    assert_eq!(user_version(&path), 1);
    assert!(
        !has_table(&path, "tags"),
        "one transaction covers each step"
    );
    assert!(!has_table(&path, "half_done"));
    assert!(has_table(&path, "notes"));

    open_with(&path, &[V1, V2]).unwrap().close();
    assert_eq!(user_version(&path), 2);
    assert_eq!(backups(&folder), ["tsugiki.db.v1.bak"]);
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn only_the_last_few_backups_are_kept() {
    let (path, folder) = temp_database("pruned");
    let steps: Vec<Migration> = (1..=6)
        .map(|version| Migration {
            version,
            sql: "CREATE TABLE IF NOT EXISTS notes (id INTEGER PRIMARY KEY);",
        })
        .collect();
    for count in 1..=steps.len() {
        open_with(&path, steps.get(..count).unwrap())
            .unwrap()
            .close();
    }
    fs::write(folder.join("tsugiki.db.notes.bak"), "someone else's").unwrap();

    open_with(&path, &steps).unwrap().close();

    assert_eq!(
        backups(&folder),
        [
            "tsugiki.db.notes.bak",
            "tsugiki.db.v3.bak",
            "tsugiki.db.v4.bak",
            "tsugiki.db.v5.bak"
        ]
    );
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn the_backup_just_written_stays_whatever_the_numbers_on_the_others() {
    let (path, folder) = temp_database("just-written");
    open_with(&path, &[V1, V2]).unwrap().close();
    for name in [
        "tsugiki.db.v3.bak",
        "tsugiki.db.v4.bak",
        "tsugiki.db.v5.bak",
    ] {
        fs::write(folder.join(name), "from a later version").unwrap();
    }

    open_with(&path, &[V1, V2, V3]).unwrap().close();

    assert_eq!(
        backups(&folder),
        [
            "tsugiki.db.v2.bak",
            "tsugiki.db.v4.bak",
            "tsugiki.db.v5.bak"
        ]
    );
    assert_eq!(user_version(&folder.join("tsugiki.db.v2.bak")), 2);
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_backup_with_a_padded_number_goes_by_its_real_name() {
    let (path, folder) = temp_database("padded");
    open_with(&path, &[V1]).unwrap().close();
    for name in [
        "tsugiki.db.v01.bak",
        "tsugiki.db.v2.bak",
        "tsugiki.db.v3.bak",
        "tsugiki.db.v4.bak",
    ] {
        fs::write(folder.join(name), "old").unwrap();
    }

    open_with(&path, &[V1, V2]).unwrap().close();

    assert_eq!(
        backups(&folder),
        [
            "tsugiki.db.v1.bak",
            "tsugiki.db.v3.bak",
            "tsugiki.db.v4.bak"
        ]
    );
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn an_old_backup_that_cant_be_removed_doesnt_stop_the_migration() {
    let (path, folder) = temp_database("stuck-backup");
    open_with(&path, &[V1]).unwrap().close();
    for version in 2..=5 {
        // Removing a file fails on a folder, on Windows and on macOS.
        fs::create_dir(folder.join(format!("tsugiki.db.v{version}.bak"))).unwrap();
    }

    open_with(&path, &[V1, V2]).unwrap().close();

    assert_eq!(user_version(&path), 2);
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_backup_that_fails_keeps_the_one_before_it() {
    let (path, folder) = temp_database("failed-backup");
    open_with(&path, &[V1]).unwrap().close();
    let earlier = folder.join("tsugiki.db.v1.bak");
    fs::write(&earlier, "the earlier backup").unwrap();
    // A folder where the new backup goes first makes writing it fail.
    fs::create_dir(folder.join("tsugiki.db.v1.bak.tmp")).unwrap();

    let problem = open_with(&path, &[V1, V2]).err().unwrap();

    assert_eq!(problem.message, CANT_UPDATE);
    assert_eq!(fs::read_to_string(&earlier).unwrap(), "the earlier backup");
    assert_eq!(user_version(&path), 1);
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_backup_left_half_written_doesnt_block_the_next_one() {
    let (path, folder) = temp_database("half-written");
    open_with(&path, &[V1]).unwrap().close();
    let partial = folder.join("tsugiki.db.v1.bak.tmp");
    fs::write(&partial, "half of a backup").unwrap();

    open_with(&path, &[V1, V2]).unwrap().close();

    assert!(!partial.exists());
    assert_eq!(user_version(&folder.join("tsugiki.db.v1.bak")), 1);
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_file_with_tables_and_no_version_gets_a_backup() {
    let (path, folder) = temp_database("no-version");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch("CREATE TABLE theirs (id INTEGER PRIMARY KEY);")
        .unwrap();
    drop(connection);

    open_with(&path, &[V1]).unwrap().close();

    assert_eq!(backups(&folder), ["tsugiki.db.v0.bak"]);
    assert!(has_table(&folder.join("tsugiki.db.v0.bak"), "theirs"));
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_newer_version_still_in_the_wal_is_refused_too() {
    let (path, folder) = temp_database("newer-in-wal");
    open_with(&path, &[V1]).unwrap().close();
    // While this stays open its change sits in the `-wal` file, and the header still says version 1.
    let writer = Connection::open(&path).unwrap();
    writer.pragma_update(None, "user_version", 2).unwrap();
    assert_eq!(contents(&path), Contents::Database { version: 1 });

    let problem = open_with(&path, &[V1]).err().unwrap();

    assert!(problem.message.contains("newer"), "{}", problem.message);
    drop(writer);
    fs::remove_dir_all(folder).unwrap();
}

/// A parent and a child that points at it.
const PARENTS: Migration = Migration {
        version: 2,
        sql: "CREATE TABLE parents (id INTEGER PRIMARY KEY);
              CREATE TABLE children (id INTEGER PRIMARY KEY, parent INTEGER NOT NULL REFERENCES parents (id));
              INSERT INTO parents (id) VALUES (1);
              INSERT INTO children (parent) VALUES (1);",
    };

/// Gives `parents` a new column the way SQLite's steps for changing a table do: a new table, a copy, a
/// drop and a rename. Dropping a parent that has children fails while foreign keys are enforced.
const REBUILT_PARENTS: Migration = Migration {
    version: 3,
    sql: "CREATE TABLE new_parents (id INTEGER PRIMARY KEY, name TEXT NOT NULL DEFAULT '');
              INSERT INTO new_parents (id) SELECT id FROM parents;
              DROP TABLE parents;
              ALTER TABLE new_parents RENAME TO parents;",
};

const ORPHAN: Migration = Migration {
    version: 3,
    sql: "INSERT INTO children (parent) VALUES (99);",
};

fn children(path: &Path) -> Vec<u32> {
    let connection = Connection::open(path).unwrap();
    let mut statement = connection
        .prepare("SELECT parent FROM children ORDER BY id")
        .unwrap();
    statement
        .query_map([], |row| row.get("parent"))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn enforces_foreign_keys(connection: &Connection) -> bool {
    connection
        .pragma_query_value(None, "foreign_keys", |row| row.get("foreign_keys"))
        .unwrap()
}

#[test]
fn a_migration_can_rebuild_a_table_that_others_point_at() {
    let (path, folder) = temp_database("rebuilt-parent");
    open_with(&path, &[V1, PARENTS]).unwrap().close();

    open_with(&path, &[V1, PARENTS, REBUILT_PARENTS])
        .unwrap()
        .close();

    assert_eq!(user_version(&path), 3);
    assert_eq!(children(&path), [1]);
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn a_migration_that_breaks_a_foreign_key_fails_and_changes_nothing() {
    let (path, folder) = temp_database("broken-key");
    open_with(&path, &[V1, PARENTS]).unwrap().close();

    let problem = open_with(&path, &[V1, PARENTS, ORPHAN]).err().unwrap();

    assert_eq!(problem.kind, ErrorKind::Internal);
    assert_eq!(problem.message, CANT_UPDATE);
    assert_eq!(user_version(&path), 2);
    assert_eq!(children(&path), [1]);
    assert_eq!(backups(&folder), ["tsugiki.db.v2.bak"]);
    assert_eq!(children(&folder.join("tsugiki.db.v2.bak")), [1]);
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn foreign_keys_are_enforced_again_after_a_migration_whatever_happened() {
    let (path, folder) = temp_database("keys-back-on");
    let mut connection = Connection::open(&path).unwrap();

    run(&mut connection, &path, &[V1, PARENTS]).unwrap();
    assert!(enforces_foreign_keys(&connection));
    let _problem = run(&mut connection, &path, &[V1, PARENTS, ORPHAN]).unwrap_err();
    assert!(enforces_foreign_keys(&connection));

    drop(connection);
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn the_migrations_count_up_from_one() {
    for (expected, step) in (1..).zip(MIGRATIONS) {
        assert_eq!(step.version, expected);
    }
}
