//! The PR view: a PR's row, and its Conversation (ADR 0008). The Files tab comes with the diff.

use std::collections::BTreeSet;

use serde::Serialize;
use specta::Type;

use super::{WorkbenchRow, candidates};
use crate::error::Error;
use crate::github::issue_content::IssueContent;
use crate::repository::RepositorySettings;
use crate::store::StoreReader;

/// A PR as the Detail pane shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct PrView {
    #[serde(flatten)]
    pub row: WorkbenchRow,
    /// Its description and comments. Nothing, until Tsugiki has read this PR from GitHub once.
    pub content: Option<IssueContent>,
}

impl PrView {
    /// Reads PR `number`, or nothing if Tsugiki doesn't hold it. This waits on the database and never on
    /// GitHub, so the view opens on what was last read (ADR 0018).
    pub fn read(
        reader: &StoreReader,
        repository: &RepositorySettings,
        number: u32,
    ) -> Result<Option<Self>, Error> {
        let (facts, content) = reader.issue_facts(&repository.name, number)?;
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
mod tests {
    use time::OffsetDateTime;

    use super::*;
    use crate::github::issue_content::IssueComment;
    use crate::github::pull_requests::PullRequestFacts;
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

    #[test]
    fn a_pr_that_fixes_no_issue_shows_what_was_last_read() {
        let world = ViewWorld::new("pr-view-after-reading");
        let standalone = PullRequestFacts {
            number: 9,
            ..stranger()
        };
        world.has(vec![standalone], vec![]);
        let content = IssueContent {
            body_html: "<p>Fixes the crash</p>".to_owned(),
            comments: vec![IssueComment {
                author: None,
                created_at: OffsetDateTime::UNIX_EPOCH,
                body_html: "<p>Looks good</p>".to_owned(),
            }],
            earlier_comments: 2,
        };
        world
            .store
            .save_issue_content(&world.settings().name, 9, content.clone())
            .unwrap();

        let view = view(&world, 9).unwrap();

        assert_eq!(view.row.number, 9);
        assert_eq!(view.content, Some(content));
        world.finish();
    }
}
