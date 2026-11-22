use crate::repository::{OpenItems, RepositorySync};
use crate::store::testing::OpenStore;
use crate::testing::{cpython, gcmon};

#[test]
fn a_new_count_replaces_the_last_one_and_says_so_for_its_own_repository_alone() {
    let db = OpenStore::new("open-items");
    let before = db.reader.repository_sync(&cpython()).unwrap();
    let first = OpenItems { issues: 7, prs: 5 };
    let second = OpenItems { issues: 8, prs: 4 };
    let saved =
        [first, first, second].map(|open| db.store.save_open_items(&cpython(), open).unwrap());
    let counted = db.reader.repository_sync(&cpython()).unwrap();
    let elsewhere = db.reader.repository_sync(&gcmon()).unwrap();
    db.close();

    let nothing = RepositorySync {
        last_sync: None,
        open: None,
    };
    assert_eq!(before, nothing);
    assert_eq!(saved, [true, false, true]);
    assert_eq!(counted.open, Some(second));
    assert_eq!(elsewhere, nothing);
}
