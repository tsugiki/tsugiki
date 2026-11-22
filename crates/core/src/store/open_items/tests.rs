use crate::repository::{OpenItems, RepositorySync};
use crate::store::testing::OpenStore;
use crate::testing::{cpython, gcmon};

#[test]
fn a_count_replaces_the_last_one_and_only_for_its_own_repository() {
    let db = OpenStore::new("open-items");
    let before = db.reader.repository_sync(&cpython()).unwrap();
    db.store
        .save_open_items(&cpython(), OpenItems { issues: 7, prs: 5 })
        .unwrap();
    db.store
        .save_open_items(&cpython(), OpenItems { issues: 8, prs: 4 })
        .unwrap();
    let counted = db.reader.repository_sync(&cpython()).unwrap();
    let elsewhere = db.reader.repository_sync(&gcmon()).unwrap();
    db.close();

    let nothing = RepositorySync {
        last_sync: None,
        open: None,
    };
    assert_eq!(before, nothing);
    assert_eq!(counted.open, Some(OpenItems { issues: 8, prs: 4 }));
    assert_eq!(elsewhere, nothing);
}
