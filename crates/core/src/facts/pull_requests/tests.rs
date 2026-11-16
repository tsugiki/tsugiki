use time::OffsetDateTime;

use super::{HeadInMirror, PrHead, PrState, PullRequestFacts};
use crate::facts::CommitSha;

fn pr_7(head: Option<&str>) -> PullRequestFacts {
    PullRequestFacts {
        number: 7,
        title: "gh-7: Fix it".to_owned(),
        state: PrState::Open,
        is_draft: false,
        author: None,
        target_branch: "3.14".to_owned(),
        ci: None,
        updated_at: OffsetDateTime::UNIX_EPOCH,
        review_requests: vec![],
        latest_reviews: vec![],
        assignees: vec![],
        labels: vec![],
        closing_issues: vec![],
        merge_commit: None,
        base: None,
        head: head.map(|sha| CommitSha::parse(sha).unwrap()),
        off_default: None,
    }
}

const HEAD: &str = "89abcde89abcde89abcde89abcde89abcde89abc";

#[test]
fn a_pr_whose_head_tsugiki_doesnt_know_has_nothing_to_look_for() {
    let looked = |_: &CommitSha| panic!("there's no head to look for");
    assert_eq!(HeadInMirror::of(&pr_7(None), looked), HeadInMirror::Unknown);
}

#[test]
fn a_head_the_mirror_lacks_names_the_branch_its_diff_starts_from() {
    let head = HeadInMirror::of(&pr_7(Some(HEAD)), |_| false);
    let wanted = PrHead {
        number: 7,
        commit: CommitSha::parse(HEAD).unwrap(),
        target_branch: "3.14".to_owned(),
    };
    assert_eq!(head, HeadInMirror::Missing(wanted));
}

#[test]
fn a_head_the_mirror_holds_needs_nothing() {
    let holds = |commit: &CommitSha| commit.as_str() == HEAD;
    assert_eq!(
        HeadInMirror::of(&pr_7(Some(HEAD)), holds),
        HeadInMirror::Held
    );
}
