//! The review threads Files shows under their code (#184). Threads that Files can't place by their line,
//! such as Outdated ones and those on a whole file, are #185's.

use serde::Serialize;
use specta::Type;

use crate::diff::{Diff, DiffFile, DiffRow};
use crate::github::issue_content::IssueComment;
use crate::github::pull_request_content::{
    Anchor, Resolution, Review, Side, Subject, ThreadId, ThreadLine,
};

/// A review thread under a row of the diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct DiffThread {
    pub id: ThreadId,
    /// The row of the line it is on, which it sits under.
    pub row: u32,
    /// Resolved also when GitHub hides each of its comments, which leaves nothing in it to answer.
    pub resolution: Resolution,
    /// Its first 100 comments, oldest first.
    pub comments: Vec<IssueComment>,
    /// The count of later comments that only GitHub holds.
    pub later_comments: u32,
}

/// The threads of `reviews` on a line of `diff`, in the order of their rows. A thread names its file as it
/// is at the head, or else as it was before a rename: a PR can rename a file and add another by its old
/// name.
pub(super) fn place(diff: &Diff, reviews: &[Review]) -> Vec<DiffThread> {
    let threads = reviews.iter().flat_map(|review| &review.threads);
    let mut placed: Vec<DiffThread> = threads
        .filter_map(|thread| {
            let all_hidden = thread
                .comments
                .iter()
                .all(|comment| comment.hidden.is_some());
            Some(DiffThread {
                id: thread.id.clone(),
                row: row_of(diff, thread)?,
                resolution: if all_hidden {
                    Resolution::Resolved
                } else {
                    thread.resolution
                },
                comments: thread.comments.clone(),
                later_comments: thread.later_comments(),
            })
        })
        .collect();
    placed.sort_by_key(|thread| thread.row);
    placed
}

/// The row of the line `thread` is on, if the diff shows it.
fn row_of(diff: &Diff, thread: &ThreadLine) -> Option<u32> {
    let Subject::Line {
        side,
        line: Some(line),
        ..
    } = thread.subject
    else {
        return None;
    };
    if thread.anchor == Anchor::Outdated {
        return None;
    }
    let files = diff.files();
    let at = files
        .iter()
        .position(|file| file.path == thread.path)
        .or_else(|| {
            let renamed = |file: &DiffFile| file.old_path.as_deref() == Some(thread.path.as_str());
            files.iter().position(renamed)
        })?;
    let start = files.get(at)?.row.checked_add(1)?;
    let end = files.get(at + 1).map_or(diff.row_count(), |next| next.row);
    let found = diff.rows(start..end).iter().position(|row| match row {
        DiffRow::Context { old, new, .. } => match side {
            Side::Left => *old == line,
            Side::Right => *new == line,
        },
        DiffRow::Added { new, .. } => side == Side::Right && *new == line,
        DiffRow::Removed { old, .. } => side == Side::Left && *old == line,
        DiffRow::File { .. } | DiffRow::Hunk { .. } | DiffRow::Note { .. } => false,
    })?;
    start.checked_add(u32::try_from(found).ok()?)
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;

    use super::*;
    use crate::github::issue_content::HiddenReason;
    use crate::github::pull_request_content::Opening;
    use crate::github::pull_requests::ReviewState;

    /// `Lib/ast.py` with one line changed, and `Lib/old.py` renamed to `Lib/new.py` with a line added.
    ///
    /// | Row | What |
    /// |---|---|
    /// | 0, 1 | `Lib/ast.py`'s header and hunk |
    /// | 2 | line 1 at both |
    /// | 3, 4 | line 2 removed, line 2 added |
    /// | 5 | line 3 at both |
    /// | 6, 7 | `Lib/new.py`'s header and hunk |
    /// | 8 | line 4 at both |
    /// | 9 | line 5 added |
    /// | 10 | line 5 at the base, 6 at the head |
    fn diff() -> Diff {
        let numstat = "1\t1\tLib/ast.py\0\
                       1\t0\t\0Lib/old.py\0Lib/new.py\0";
        let patch = "diff --git a/Lib/ast.py b/Lib/ast.py\n\
                     @@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n\
                     diff --git a/Lib/old.py b/Lib/new.py\n\
                     @@ -4,2 +4,3 @@\n d\n+e\n f\n";
        Diff::parse(numstat, patch).unwrap()
    }

    fn comment(hidden: Option<HiddenReason>) -> IssueComment {
        IssueComment {
            author: None,
            created_at: OffsetDateTime::UNIX_EPOCH,
            body_html: "<p>Why?</p>".to_owned(),
            hidden,
        }
    }

    /// An open thread `id` on line `line` of `path`, on `side`, with one comment.
    fn thread(id: &str, path: &str, side: Side, line: u32) -> ThreadLine {
        ThreadLine {
            id: ThreadId::new(id),
            path: path.to_owned(),
            opening: Opening::Words {
                words: "Why?".to_owned(),
            },
            replies: 0,
            resolution: Resolution::Open,
            anchor: Anchor::Current,
            subject: Subject::Line {
                side,
                line: Some(line),
                original: Some(line),
            },
            comments: vec![comment(None)],
        }
    }

    fn reviews(threads: Vec<ThreadLine>) -> Vec<Review> {
        vec![Review {
            author: None,
            state: ReviewState::Commented,
            commit: None,
            submitted_at: OffsetDateTime::UNIX_EPOCH,
            body_html: String::new(),
            hidden: None,
            threads,
        }]
    }

    fn rows(placed: &[DiffThread]) -> Vec<(&str, u32)> {
        placed
            .iter()
            .map(|thread| (thread.id.as_str(), thread.row))
            .collect()
    }

    #[test]
    fn a_thread_sits_under_its_line_at_the_head_or_at_the_merge_base_in_row_order() {
        let threads = vec![
            thread("added", "Lib/ast.py", Side::Right, 2),
            thread("removed", "Lib/ast.py", Side::Left, 2),
            thread("context at the head", "Lib/new.py", Side::Right, 6),
            thread("context at the base", "Lib/ast.py", Side::Left, 3),
        ];

        let placed = place(&diff(), &reviews(threads));

        assert_eq!(
            rows(&placed),
            [
                ("removed", 3),
                ("added", 4),
                ("context at the base", 5),
                ("context at the head", 10)
            ]
        );
    }

    #[test]
    fn a_renamed_file_s_threads_name_it_either_way() {
        let threads = vec![
            thread("new name", "Lib/new.py", Side::Right, 5),
            thread("old name", "Lib/old.py", Side::Left, 4),
        ];

        let placed = place(&diff(), &reviews(threads));

        assert_eq!(rows(&placed), [("old name", 8), ("new name", 9)]);
    }

    #[test]
    fn a_thread_on_a_file_that_took_a_renamed_file_s_old_name_stays_on_it() {
        // `Lib/a.py` moved to `Lib/b.py`, and a new `Lib/a.py` came after it.
        let numstat = "1\t0\t\0Lib/a.py\0Lib/b.py\0\
                       1\t0\tLib/a.py\0";
        let patch = "diff --git a/Lib/a.py b/Lib/b.py\n\
                     @@ -1 +1,2 @@\n x\n+y\n\
                     diff --git a/Lib/a.py b/Lib/a.py\n\
                     @@ -0,0 +1,2 @@\n+new\n+file\n";
        let diff = Diff::parse(numstat, patch).unwrap();
        let threads = vec![
            thread("new file", "Lib/a.py", Side::Right, 2),
            thread("before the rename", "Lib/a.py", Side::Left, 1),
        ];

        let placed = place(&diff, &reviews(threads));

        // The rename's rows are 0 to 3, and the new file's 4 to 7.
        assert_eq!(rows(&placed), [("new file", 7)]);
    }

    #[test]
    fn a_thread_files_cant_place_by_its_line_waits_for_185() {
        let mut outdated = thread("outdated", "Lib/ast.py", Side::Right, 1);
        outdated.anchor = Anchor::Outdated;
        let mut on_the_file = thread("file", "Lib/ast.py", Side::Right, 1);
        on_the_file.subject = Subject::File;
        let mut moved = thread("moved", "Lib/ast.py", Side::Right, 1);
        moved.subject = Subject::Line {
            side: Side::Right,
            line: None,
            original: Some(1),
        };
        let threads = vec![
            outdated,
            on_the_file,
            moved,
            thread("outside the hunks", "Lib/ast.py", Side::Right, 9),
            thread("an added line at the base", "Lib/new.py", Side::Left, 6),
            thread("another file", "Lib/re.py", Side::Right, 1),
        ];

        assert_eq!(place(&diff(), &reviews(threads)), []);
    }

    #[test]
    fn a_thread_of_only_hidden_comments_counts_as_resolved() {
        let mut hidden = thread("hidden", "Lib/ast.py", Side::Right, 1);
        hidden.comments = vec![comment(Some(HiddenReason::Spam))];
        let mut partly = thread("partly", "Lib/ast.py", Side::Right, 3);
        partly.comments = vec![comment(Some(HiddenReason::Spam)), comment(None)];
        partly.replies = 4;

        let placed = place(&diff(), &reviews(vec![hidden, partly]));

        let marks: Vec<(Resolution, u32)> = placed
            .iter()
            .map(|thread| (thread.resolution, thread.later_comments))
            .collect();
        assert_eq!(marks, [(Resolution::Resolved, 0), (Resolution::Open, 3)]);
    }
}
