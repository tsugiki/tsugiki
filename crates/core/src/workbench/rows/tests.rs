use crate::facts::pull_requests::{CiState, PrState, PullRequestFacts};
use crate::work::{SeenMark, WorkReason, YourReview};
use crate::workbench::testing::{at, me, mine, workbench};
use crate::workbench::{MuteAuthor, PrTree, Seen, SeenState, WorkbenchItem, WorkbenchRow};

#[test]
fn a_row_carries_what_the_list_shows() {
    let found = workbench(
        vec![PullRequestFacts {
            is_draft: true,
            ci: Some(CiState::Pending),
            updated_at: at(3),
            ..mine(3)
        }],
        vec![],
    );
    assert_eq!(
        found.items[0].item,
        WorkbenchItem::StandalonePr(PrTree {
            row: WorkbenchRow {
                number: 3,
                title: "PR 3".to_owned(),
                author: Some(me()),
                state: PrState::Open,
                is_draft: true,
                target_branch: "main".to_owned(),
                ci: Some(CiState::Pending),
                updated_at: at(3),
                reasons: vec![WorkReason::OwnPr],
                your_review: YourReview::Unreviewed,
                seen: Seen {
                    mark: SeenMark {
                        numbers: vec![3],
                        up_to: Some(at(3)),
                    },
                    state: SeenState::Seen,
                },
                mute_author: MuteAuthor::Off,
            },
            backports: vec![],
        })
    );
}
