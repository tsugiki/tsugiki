//! Where Files shows each review thread: under its line (#184), at the top of its file when it is on the
//! whole file or Outdated, in the gap around a line no hunk shows, or after the last file when the diff no
//! longer holds its file (#185). The threads come in the order Files shows them, which `F8` follows.

use serde::Serialize;
use specta::Type;

use crate::diff::{Diff, DiffFile, DiffRow};
use crate::github::pull_request_content::{
    Anchor, CodeRow, DiffHunk, Resolution, Side, Subject, ThreadComment, ThreadId, ThreadLine,
};
use crate::highlight::{Language, Parsed, Span};

/// A review thread, and where Files shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct DiffThread {
    pub id: ThreadId,
    pub at: Placement,
    /// Resolved also when GitHub hides each of its comments, which leaves nothing in it to answer.
    pub resolution: Resolution,
    /// Its first 100 comments, oldest first, with the user's drafts among them.
    pub comments: Vec<ThreadComment>,
    /// The count of later comments that only GitHub holds.
    pub later_comments: u32,
}

/// Where Files shows a thread. A file is named by its place among the diff's files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Placement {
    /// Under the diff's row `row`, which shows its line.
    Row { row: u32 },
    /// At the top of file `file`, as it is on the whole file, above the file's Outdated threads.
    File { file: u32 },
    /// At the top of file `file`, as the code under it has changed since, with the code it was written
    /// against.
    Outdated { file: u32, code: Option<ThreadCode> },
    /// Under line `head_line` of file `file` at the head, which no hunk shows, so the gap or the end of the
    /// file around it opens to show it. `line` is the line as the thread's side counts it.
    Outside {
        file: u32,
        line: u32,
        head_line: u32,
    },
    /// After the last file, on file `path`, which the diff no longer holds, with the code it was written
    /// against.
    Gone {
        path: String,
        code: Option<ThreadCode>,
    },
}

/// The code a thread was written against, from the hunk GitHub stored with it: its rows, the last of them
/// the thread's line, and the spans that highlight each row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ThreadCode {
    pub rows: Vec<CodeRow>,
    pub spans: Vec<Vec<Span>>,
}

/// Where a thread comes among the others: the diff's row it follows, its rank among the threads after that
/// row, its line at the head, and for a thread on a file the diff no longer holds, that file's name.
type Order = (u32, u8, u32, String);

/// `threads` in the order Files shows them. In each file come the threads on the file, then the Outdated
/// ones, then those on its lines; after the last file, the threads on files the diff no longer holds, by
/// file.
pub(super) fn place<'a>(
    diff: &Diff,
    threads: impl Iterator<Item = &'a ThreadLine>,
) -> Vec<DiffThread> {
    let mut placed: Vec<(Order, DiffThread)> = threads
        .map(|thread| {
            let all_hidden = thread
                .comments
                .iter()
                .all(|comment| comment.comment.hidden.is_some());
            let (order, at) = placement(diff, thread);
            let shown = DiffThread {
                id: thread.id.clone(),
                at,
                resolution: if all_hidden {
                    Resolution::Resolved
                } else {
                    thread.resolution
                },
                comments: thread.comments.clone(),
                later_comments: thread.later_comments(),
            };
            (order, shown)
        })
        .collect();
    placed.sort_by(|(one, _), (other, _)| one.cmp(other));
    placed.into_iter().map(|(_, thread)| thread).collect()
}

/// Where Files shows `thread`, and where that comes among the others. A thread names its file as it is at
/// the head, or else as it was before a rename: a PR can rename a file and add another by its old name.
fn placement(diff: &Diff, thread: &ThreadLine) -> (Order, Placement) {
    let files = diff.files();
    let named =
        |matches: &dyn Fn(&DiffFile) -> bool| (0_u32..).zip(files).find(|(_, file)| matches(file));
    let found = named(&|file| file.path == thread.path)
        .or_else(|| named(&|file| file.old_path.as_deref() == Some(thread.path.as_str())));
    let code = || thread.hunk.as_ref().map(|hunk| code_of(hunk, &thread.path));
    let Some((index, file)) = found else {
        let order = (u32::MAX, 0, 0, thread.path.clone());
        let path = thread.path.clone();
        return (order, Placement::Gone { path, code: code() });
    };
    let end = files
        .iter()
        .find(|next| next.row > file.row)
        .map_or(diff.row_count(), |next| next.row);
    let outdated = || {
        let at = Placement::Outdated {
            file: index,
            code: code(),
        };
        ((file.row, 1, 0, String::new()), at)
    };
    let (side, line) = match thread.subject {
        Subject::File => {
            return (
                (file.row, 0, 0, String::new()),
                Placement::File { file: index },
            );
        }
        Subject::Line {
            side,
            line: Some(line),
            ..
        } if thread.anchor == Anchor::Current => (side, line),
        Subject::Line { .. } => return outdated(),
    };
    if let Some(row) = row_of(diff, file.row.saturating_add(1)..end, side, line) {
        return ((row, 2, 0, String::new()), Placement::Row { row });
    }
    // A line in no row, no gap and not after the last hunk has nowhere to show, so it counts as Outdated.
    let Some((row, rank, head_line)) = outside(file, end, side, line) else {
        return outdated();
    };
    let at = Placement::Outside {
        file: index,
        line,
        head_line,
    };
    ((row, rank, head_line, String::new()), at)
}

/// The row among `rows` that shows line `line` of `side`, if one does.
pub(super) fn row_of(
    diff: &Diff,
    rows: std::ops::Range<u32>,
    side: Side,
    line: u32,
) -> Option<u32> {
    let start = rows.start;
    let found = diff.rows(rows).iter().position(|row| match row {
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

/// The gap or the end of `file` that holds line `line` of `side`, which no hunk shows: the row the thread
/// comes before or after, its rank there, and the line at the head. A gap's lines show above its hunk's
/// rows, and the end of a file after its last row, whose rows end at `end`.
fn outside(file: &DiffFile, end: u32, side: Side, line: u32) -> Option<(u32, u8, u32)> {
    let on_side = |old: u32, new: u32| match side {
        Side::Left => old,
        Side::Right => new,
    };
    for gap in &file.gaps {
        let first = on_side(gap.old, gap.new);
        if (first..first.saturating_add(gap.lines)).contains(&line) {
            return Some((gap.row, 2, gap.new.saturating_add(line - first)));
        }
    }
    let rest = file.rest?;
    let first = on_side(rest.old, rest.new);
    let head_line = rest.new.saturating_add(line.checked_sub(first)?);
    Some((end.saturating_sub(1), 3, head_line))
}

/// The rows of `hunk`, highlighted as code of file `path`.
fn code_of(hunk: &DiffHunk, path: &str) -> ThreadCode {
    let rows = hunk.rows().to_vec();
    // The code of the rows after the `@@` line, one line each, which parses as one piece.
    let mut source = String::new();
    for row in rows.iter().skip(1) {
        if let CodeRow::Context { text, .. }
        | CodeRow::Added { text, .. }
        | CodeRow::Removed { text, .. } = row
        {
            source.push_str(text);
        }
        source.push('\n');
    }
    let count = u32::try_from(rows.len()).unwrap_or(u32::MAX);
    let parsed = Language::of(path).and_then(|language| Parsed::parse(language, source));
    // The `@@` line has no code, so the code's line 1 is row 1.
    let mut spans = vec![Vec::new()];
    spans.extend(parsed.map_or_else(Vec::new, |parsed| parsed.spans(1..count)));
    spans.resize(rows.len(), Vec::new());
    ThreadCode { rows, spans }
}

#[cfg(test)]
mod tests;
