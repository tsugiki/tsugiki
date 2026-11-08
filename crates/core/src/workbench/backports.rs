//! Planting each Backport under its Origin PR, so a tree stays two levels (ADR 0011).

use std::collections::{BTreeMap, BTreeSet};

use super::links::Linked;
use super::{BackportRow, PrTree, TreeEntry, WorkbenchItem, WorkbenchRow};
use crate::links::BackportKind;

/// A Backport beneath its Origin PR.
struct Beneath {
    row: BackportRow,
    issues: BTreeSet<u32>,
}

/// The two shapes of a tree's top line.
enum Shape {
    Pr {
        beneath: Vec<Beneath>,
    },
    /// A Backport with no Origin PR to sit under.
    Backport(BackportKind),
}

/// A line that sits directly under an Issue or on its own.
pub(super) struct Top {
    row: WorkbenchRow,
    shape: Shape,
    /// Each Issue it shows under: its own, and those of the Backports beneath it (ADR 0011).
    pub(super) issues: BTreeSet<u32>,
}

impl Top {
    /// A Backport shows under an Issue it fixes. One that fixes none follows its Origin PR.
    pub(super) fn entry_under(&self, issue: u32) -> TreeEntry {
        let row = self.row.clone();
        match &self.shape {
            Shape::Backport(backport) => TreeEntry::Backport(BackportRow {
                row,
                backport: *backport,
            }),
            Shape::Pr { beneath } => TreeEntry::Pr(PrTree {
                row,
                backports: beneath
                    .iter()
                    .filter(|below| below.issues.is_empty() || below.issues.contains(&issue))
                    .map(|below| below.row.clone())
                    .collect(),
            }),
        }
    }
}

/// Puts each Backport beneath its Origin PR when Tsugiki has that PR and it isn't a Backport too, which keeps
/// a tree two levels deep. The order of `linked` stays within each level.
pub(super) fn plant(linked: Vec<Linked>) -> Vec<Top> {
    let origins: BTreeSet<u32> = linked
        .iter()
        .filter(|pr| pr.backport.is_none())
        .map(|pr| pr.row.number)
        .collect();
    let mut beneath: BTreeMap<u32, Vec<Beneath>> = BTreeMap::new();
    let mut tops = Vec::new();
    for pr in linked {
        match pr.backport {
            Some(link) if origins.contains(&link.origin) => {
                beneath.entry(link.origin).or_default().push(Beneath {
                    row: BackportRow {
                        row: pr.row,
                        backport: link.kind,
                    },
                    issues: pr.issues,
                });
            }
            _ => tops.push(Top {
                row: pr.row,
                shape: pr.backport.map_or_else(
                    || Shape::Pr {
                        beneath: Vec::new(),
                    },
                    |link| Shape::Backport(link.kind),
                ),
                issues: pr.issues,
            }),
        }
    }
    for top in &mut tops {
        if let Shape::Pr { beneath: place } = &mut top.shape
            && let Some(below) = beneath.remove(&top.row.number)
        {
            top.issues
                .extend(below.iter().flat_map(|b| b.issues.iter().copied()));
            *place = below;
        }
    }
    tops
}

/// The item for a line with no Issue. Nothing beneath it fixes an Issue, or the line would show under that
/// Issue.
pub(super) fn standalone(top: Top) -> WorkbenchItem {
    match top.shape {
        Shape::Backport(backport) => WorkbenchItem::StandaloneBackport(BackportRow {
            row: top.row,
            backport,
        }),
        Shape::Pr { beneath } => WorkbenchItem::StandalonePr(PrTree {
            row: top.row,
            backports: beneath.into_iter().map(|below| below.row).collect(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use crate::github::CommitSha;
    use crate::github::pull_requests::OffDefault;
    use crate::github::pull_requests::{PrState, PullRequestFacts};
    use crate::links::BackportKind;
    use crate::workbench::testing::{
        at, backport, bots_backport, first_issue, fixing, issue, me, mine, outline, own, stranger,
        workbench,
    };
    use crate::workbench::{TreeEntry, Workbench};

    /// The first Issue's lines, as each PR's number with the numbers beneath it.
    fn tree_of(workbench: &Workbench) -> Vec<(u32, Vec<u32>)> {
        first_issue(workbench)
            .pull_requests
            .iter()
            .map(|entry| match entry {
                TreeEntry::Pr(tree) => (
                    tree.row.number,
                    tree.backports
                        .iter()
                        .map(|below| below.row.number)
                        .collect(),
                ),
                TreeEntry::Backport(backport) => (backport.row.number, vec![]),
            })
            .collect()
    }

    #[test]
    fn the_window_gets_a_backports_kind_beside_its_row() {
        let found = workbench(
            vec![
                fixing(4, 1),
                backport(5, "[3.14] gh-1: Fix it (GH-4)", vec![]),
                backport(6, "[3.13] gh-1: Fix it (GH-99)", vec![]),
            ],
            vec![issue(1)],
        );
        let sent = serde_json::to_value(&found).unwrap();
        let entries = &sent["quiet"][0]["pull_requests"];
        assert_eq!(entries[0]["backports"][0]["number"], 5);
        assert_eq!(entries[0]["backports"][0]["backport"], "manual");
        assert_eq!(entries[1]["kind"], "backport");
        assert_eq!(entries[1]["number"], 6);
        assert_eq!(entries[1]["backport"], "manual");
    }

    #[test]
    fn a_backport_sits_under_its_origin_pr_with_its_kind() {
        let found = workbench(
            vec![
                fixing(10, 1),
                bots_backport(11, "[3.14] gh-1: Fix it (GH-10)"),
                backport(12, "[3.13] gh-1: Fix it (GH-10)", vec![]),
                bots_backport(13, "[3.12] gh-1: Fix it (GH-99)"),
            ],
            vec![issue(1)],
        );
        assert_eq!(tree_of(&found), [(10, vec![11, 12]), (13, vec![])]);
        let kinds: Vec<_> = first_issue(&found)
            .pull_requests
            .iter()
            .flat_map(|entry| match entry {
                TreeEntry::Pr(tree) => tree.backports.clone(),
                TreeEntry::Backport(backport) => vec![backport.clone()],
            })
            .map(|row| row.backport)
            .collect();
        assert_eq!(
            kinds,
            [BackportKind::Bot, BackportKind::Manual, BackportKind::Bot]
        );
    }

    #[test]
    fn a_backport_under_a_cherry_pick_line_finds_its_origin_pr() {
        let sha = CommitSha::parse("0123abc").unwrap();
        let origin = PullRequestFacts {
            state: PrState::Merged,
            merge_commit: Some(sha.clone()),
            head: None,
            ..fixing(10, 1)
        };
        let by_hand = PullRequestFacts {
            off_default: Some(OffDefault {
                description: String::new(),
                cherry_picked_from: vec![sha],
                committers: vec![],
            }),
            ..own(backport(11, "gh-1: Fix it on 3.14", vec![]))
        };
        let found = workbench(vec![origin, by_hand], vec![issue(1)]);
        assert_eq!(tree_of(&found), [(10, vec![11])]);
    }

    #[test]
    fn a_backport_of_another_issues_pr_brings_that_pr_under_its_own_issue() {
        let origin = fixing(10, 1);
        let found = workbench(
            vec![origin, backport(11, "[3.14] gh-2: Fix it (GH-10)", vec![])],
            vec![issue(1), issue(2)],
        );
        assert_eq!(
            outline(&found),
            [("issue", 2, vec![10, 11]), ("issue", 1, vec![10])]
        );
    }

    #[test]
    fn a_backport_that_fixes_no_issue_follows_its_origin_pr() {
        let both = PullRequestFacts {
            closing_issues: vec![2],
            ..fixing(10, 1)
        };
        let found = workbench(
            vec![both, backport(11, "[3.14] Fix it (GH-10)", vec![])],
            vec![issue(1), issue(2)],
        );
        assert_eq!(
            outline(&found),
            [("issue", 2, vec![10, 11]), ("issue", 1, vec![10, 11])]
        );
    }

    #[test]
    fn a_backport_of_a_backport_stands_by_itself_so_a_tree_stays_two_levels() {
        let found = workbench(
            vec![
                fixing(10, 1),
                backport(11, "[3.14] gh-1: Fix it (GH-10)", vec![]),
                backport(12, "[3.13] gh-1: Fix it (GH-11)", vec![]),
            ],
            vec![issue(1)],
        );
        assert_eq!(tree_of(&found), [(10, vec![11]), (12, vec![])]);
    }

    #[test]
    fn a_users_backport_lists_an_origin_pr_that_is_not_the_users() {
        let found = workbench(
            vec![
                PullRequestFacts {
                    number: 10,
                    title: "Fix it".to_owned(),
                    ..stranger()
                },
                own(backport(11, "[3.14] Fix it (GH-10)", vec![])),
            ],
            vec![],
        );
        assert_eq!(outline(&found), [("standalone", 10, vec![11])]);
    }

    #[test]
    fn a_backport_standing_by_itself_takes_its_place_by_its_time() {
        let later = PullRequestFacts {
            updated_at: at(9),
            ..own(backport(11, "[3.14] Fix it (GH-10)", vec![]))
        };
        let found = workbench(vec![mine(20), later], vec![]);
        assert_eq!(
            outline(&found),
            [
                ("standalone backport", 11, vec![]),
                ("standalone", 20, vec![])
            ]
        );
    }

    #[test]
    fn someone_elses_backport_with_no_origin_pr_and_no_issue_isnt_listed() {
        let found = workbench(vec![backport(11, "[3.14] Fix it (GH-10)", vec![])], vec![]);
        assert_eq!(outline(&found), []);
    }

    #[test]
    fn a_review_request_on_a_backport_lists_its_issue() {
        let requested = PullRequestFacts {
            review_requests: vec![me()],
            ..backport(11, "[3.14] gh-1: Fix it (GH-99)", vec![])
        };
        let found = workbench(vec![requested], vec![issue(1)]);
        assert_eq!(outline(&found), [("issue", 1, vec![11])]);
    }

    #[test]
    fn a_tree_holds_merged_and_closed_prs_and_says_which_they_are() {
        let in_state = |number, state| PullRequestFacts {
            number,
            state,
            title: "gh-1: Fix it".to_owned(),
            ..stranger()
        };
        let found = workbench(
            vec![
                fixing(10, 1),
                in_state(11, PrState::Merged),
                in_state(12, PrState::Closed),
            ],
            vec![issue(1)],
        );
        let states: Vec<_> = first_issue(&found)
            .pull_requests
            .iter()
            .flat_map(TreeEntry::rows)
            .map(|row| row.state)
            .collect();
        assert_eq!(states, [PrState::Open, PrState::Merged, PrState::Closed]);
    }
}
