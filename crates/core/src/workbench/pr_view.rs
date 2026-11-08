//! The PR view: a PR's row, and its Conversation (ADR 0008). The Files tab comes with the diff.

use std::collections::BTreeSet;

use serde::Serialize;
use specta::Type;

use super::{WorkbenchRow, candidates};
use crate::error::Error;
use crate::github::pull_request_content::PullRequestContent;
use crate::repository::RepositorySettings;
use crate::store::StoreReader;

/// A PR as the Detail pane shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct PrView {
    #[serde(flatten)]
    pub row: WorkbenchRow,
    /// Its description, comments and Reviews. Nothing, until Tsugiki has read this PR from GitHub once.
    pub content: Option<PullRequestContent>,
}

impl PrView {
    /// Reads PR `number`, or nothing if Tsugiki doesn't hold it. This waits on the database and never on
    /// GitHub, so the view opens on what was last read (ADR 0018).
    pub fn read(
        reader: &StoreReader,
        repository: &RepositorySettings,
        number: u32,
    ) -> Result<Option<Self>, Error> {
        let (facts, content) = reader.pull_request_facts(&repository.name, number)?;
        let row = candidates(facts, repository, &BTreeSet::new())
            .iter()
            .find_map(|item| {
                item.rows()
                    .into_iter()
                    .find(|row| row.number == number)
                    .cloned()
            });
        Ok(row.map(|row| Self { row, content }))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use time::OffsetDateTime;

    use super::*;
    use crate::github::issue_content::IssueComment;
    use crate::github::issue_content::{HiddenReason, IssueContent};
    use crate::github::pull_request_content::{
        Anchor, Opening, Resolution, Review, Side, Subject, ThreadId, ThreadLine,
    };
    use crate::github::pull_requests::{PullRequestFacts, ReviewState};
    use crate::github::{CommitSha, Login};
    use crate::workbench::testing::{ViewWorld, fixing, issue, stranger};

    fn view(world: &ViewWorld, number: u32) -> Option<PrView> {
        PrView::read(&world.reader, &world.settings(), number).unwrap()
    }

    #[test]
    fn a_pr_tsugiki_doesnt_hold_has_no_view() {
        let world = ViewWorld::new("pr-view-none");
        world.has(vec![], vec![issue(1)]);

        assert_eq!(view(&world, 1), None, "an Issue is no PR");
        assert_eq!(view(&world, 5), None);
        world.finish();
    }

    #[test]
    fn a_pr_in_an_issues_tree_shows_its_row_before_tsugiki_has_read_its_conversation() {
        let world = ViewWorld::new("pr-view-before-reading");
        world.has(vec![fixing(5, 1)], vec![issue(1)]);

        let view = view(&world, 5).unwrap();

        assert_eq!(view.row.number, 5);
        assert_eq!(view.row.title, "gh-1: Fix it");
        assert_eq!(view.content, None);
        world.finish();
    }

    /// A PR's Conversation with two Reviews, oldest first, one of them hidden and with two threads.
    pub(crate) fn reviewed() -> PullRequestContent {
        let thread = |line: Option<u32>, opening| ThreadLine {
            id: ThreadId::new(&format!("T{line:?}")),
            path: "Lib/ast.py".to_owned(),
            opening,
            replies: 1,
            resolution: Resolution::Open,
            anchor: if line.is_none() {
                Anchor::Outdated
            } else {
                Anchor::Current
            },
            subject: Subject::Line {
                side: Side::Right,
                line,
                original: Some(7),
            },
            comments: vec![IssueComment {
                author: Some(Login::new("some-reviewer")),
                created_at: OffsetDateTime::UNIX_EPOCH,
                body_html: "<p>Why?</p>".to_owned(),
                hidden: line.is_none().then_some(HiddenReason::Abuse),
            }],
        };
        PullRequestContent {
            conversation: IssueContent {
                body_html: "<p>Fixes the crash</p>".to_owned(),
                comments: vec![IssueComment {
                    author: None,
                    created_at: OffsetDateTime::UNIX_EPOCH,
                    body_html: "<p>Looks good</p>".to_owned(),
                    hidden: Some(HiddenReason::OffTopic),
                }],
                earlier_comments: 2,
            },
            reviews: vec![
                Review {
                    author: Some(Login::new("some-reviewer")),
                    state: ReviewState::ChangesRequested,
                    commit: CommitSha::parse("0123abc"),
                    submitted_at: OffsetDateTime::UNIX_EPOCH,
                    body_html: "<p>Some notes</p>".to_owned(),
                    hidden: Some(HiddenReason::Spam),
                    threads: vec![
                        thread(
                            Some(12),
                            Opening::Words {
                                words: "Why not a set?".to_owned(),
                            },
                        ),
                        thread(
                            None,
                            Opening::Hidden {
                                reason: HiddenReason::Abuse,
                            },
                        ),
                    ],
                },
                Review {
                    author: None,
                    state: ReviewState::Approved,
                    commit: None,
                    submitted_at: OffsetDateTime::UNIX_EPOCH + time::Duration::hours(1),
                    body_html: String::new(),
                    hidden: None,
                    threads: vec![],
                },
            ],
            earlier_reviews: 3,
            head: CommitSha::parse("0123abc"),
        }
    }

    #[test]
    fn a_pr_shows_its_reviews_oldest_first_with_their_threads_as_last_read() {
        let world = ViewWorld::new("pr-view-after-reading");
        let standalone = PullRequestFacts {
            number: 9,
            ..stranger()
        };
        world.has(vec![standalone], vec![]);
        world
            .store
            .save_pull_request_content(&world.settings().name, 9, reviewed())
            .unwrap();

        let view = view(&world, 9).unwrap();

        assert_eq!(view.row.number, 9);
        assert_eq!(view.content, Some(reviewed()));
        world.finish();
    }

    #[test]
    fn each_read_of_a_pr_replaces_its_reviews() {
        let world = ViewWorld::new("pr-view-read-again");
        world.has(
            vec![PullRequestFacts {
                number: 9,
                ..stranger()
            }],
            vec![],
        );
        let name = world.settings().name;
        world
            .store
            .save_pull_request_content(&name, 9, reviewed())
            .unwrap();
        let mut again = reviewed();
        drop(again.reviews.remove(0));
        again.earlier_reviews = 0;

        world
            .store
            .save_pull_request_content(&name, 9, again.clone())
            .unwrap();

        assert_eq!(view(&world, 9).unwrap().content, Some(again));
        world.finish();
    }

    #[test]
    fn a_pr_read_before_tsugiki_read_reviews_has_none_until_the_next_read() {
        let world = ViewWorld::new("pr-view-no-reviews-yet");
        world.has(
            vec![PullRequestFacts {
                number: 9,
                ..stranger()
            }],
            vec![],
        );
        let conversation = reviewed().conversation;
        world
            .store
            .save_issue_content(&world.settings().name, 9, conversation.clone())
            .unwrap();

        let content = view(&world, 9).unwrap().content.unwrap();

        assert_eq!(content.conversation, conversation);
        assert_eq!((content.reviews, content.earlier_reviews), (vec![], 0));
        world.finish();
    }
}
