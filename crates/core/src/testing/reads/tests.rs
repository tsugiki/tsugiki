use super::*;
use crate::store::testing::World;
use crate::testing::{cpython, cpython_settings, issue_facts, pr_facts};
use crate::workbench::inbox::InboxPage;

#[test]
fn each_read_answers_from_the_database_it_opened() {
    let world = World::new("testing-reads");
    world.has(vec![pr_facts(7)]);
    (world.db.store)
        .save_issues(&cpython(), vec![issue_facts(8)])
        .unwrap();
    let reads = Reads::open(&world.db.path).unwrap();
    let settings = cpython_settings();

    let facts = reads.work_facts(&cpython()).unwrap();
    let workbench = reads.workbench(&settings, &WorkbenchView::Whole, ListOrder::Attention);
    let inbox = reads.inbox(
        &settings,
        InboxAsk {
            page: InboxPage::Latest,
            search: "",
        },
        OffsetDateTime::UNIX_EPOCH,
    );    let issue = reads.issue_view(&settings, 8).unwrap();
    let pr = reads.pr_view(&settings, 7).unwrap();
    let found = reads.quick_open(&cpython(), "#7").unwrap();
    reads.close();
    world.finish();

    assert_eq!(facts.pull_requests, vec![pr_facts(7)]);
    assert!(workbench.is_ok() && inbox.is_ok());
    assert_eq!(issue.map(|view| view.issue.number), Some(8));
    assert_eq!(pr.map(|view| view.row.number), Some(7));
    assert_eq!(found.named.map(|item| item.number), Some(7));
}
