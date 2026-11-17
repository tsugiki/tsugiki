use super::{HeadInMirror, PrHead, PullRequestFacts};
use crate::facts::CommitSha;
use crate::testing::{pr_facts, sha};

fn pr_7(head: Option<&str>) -> PullRequestFacts {
    PullRequestFacts {
        title: "gh-7: Fix it".to_owned(),
        target_branch: "3.14".to_owned(),
        head: head.map(sha),
        ..pr_facts(7)
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
