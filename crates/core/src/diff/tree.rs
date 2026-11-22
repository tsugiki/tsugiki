//! A PR's changed files as a tree of folders, as the rail's Files pane draws it (#604). The core builds it
//! with the file list, so the window only draws the rows: it folds and filters them, and builds nothing.

use std::collections::HashMap;

use serde::Serialize;
use specta::Type;

/// Past this many files a tree's folders start folded, so a large PR opens on its top folders.
pub const FOLD_PAST: usize = 300;

/// A PR's changed files as a tree of folders: its rows in the order they show unfolded, each folder before
/// what it holds. Files and folders keep the order of the diff's files, so the tree reads down as the Files
/// tab does.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Type)]
pub struct FileTree {
    pub rows: Vec<TreeRow>,
    /// Whether its folders start folded, as they do past [`FOLD_PAST`] files.
    pub folded: bool,
}

/// A row of the tree: a folder, or a file of the diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TreeRow {
    /// A folder, joined with the folders inside it while each holds only one, as GitHub joins them:
    /// `Lib/test`.
    Folder {
        /// Its path from the Repository's top, which names it while the folds change.
        path: String,
        /// What its row shows: its name, and those of the folders joined to it.
        name: String,
        /// How many folders it sits in.
        depth: u32,
        /// The row after the last one it holds.
        end: u32,
    },
    /// A file of the diff.
    File {
        /// Its place among the diff's files.
        file: u32,
        name: String,
        /// How many folders it sits in.
        depth: u32,
        /// The review threads on it that others can read.
        threads: u32,
        /// The drafts of the user's Pending review on it, which only they see.
        drafts: u32,
    },
}

/// What a file holds besides its lines: review threads, and drafts of the user's Pending review.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileNotes {
    pub threads: u32,
    pub drafts: u32,
}

impl FileTree {
    /// The tree of the files at `paths`, in the diff's order, each with its notes in `notes` by its place.
    /// A file `notes` doesn't reach holds none.
    pub fn new<'a>(paths: impl IntoIterator<Item = &'a str>, notes: &[FileNotes]) -> Self {
        let parts: Vec<Vec<&str>> = (paths.into_iter())
            .map(|path| path.split('/').collect())
            .collect();
        let entries = (0_u32..)
            .zip(&parts)
            .map(|(file, parts)| (file, parts.as_slice()));
        let mut drawn = Drawn {
            notes,
            rows: Vec::new(),
        };
        drawn.draw(entries.collect(), "", 0);
        Self {
            rows: drawn.rows,
            folded: parts.len() > FOLD_PAST,
        }
    }
}

/// The rows drawn so far, and the notes each file's row takes by the file's place.
struct Drawn<'n> {
    notes: &'n [FileNotes],
    rows: Vec<TreeRow>,
}

/// A file, by its place among the diff's files, and the parts of its path below the folder being drawn.
type Entry<'p, 'a> = (u32, &'p [&'a str]);

/// What a folder holds: a file, or a folder and the entries inside it.
enum Child<'p, 'a> {
    File {
        file: u32,
        name: &'a str,
    },
    Folder {
        name: &'a str,
        inside: Vec<Entry<'p, 'a>>,
    },
}

impl Drawn<'_> {
    /// Draws the rows of `entries`, the files below the folder at `path`, `depth` folders down. Each file
    /// and folder comes where the diff first reaches it.
    fn draw(&mut self, entries: Vec<Entry<'_, '_>>, path: &str, depth: u32) {
        for child in children(entries) {
            match child {
                Child::File { file, name } => {
                    let held = usize::try_from(file).ok().and_then(|at| self.notes.get(at));
                    let held = held.copied().unwrap_or_default();
                    self.rows.push(TreeRow::File {
                        file,
                        name: name.to_owned(),
                        depth,
                        threads: held.threads,
                        drafts: held.drafts,
                    });
                }
                Child::Folder { name, inside } => {
                    let (name, inside) = joined(name, inside);
                    let path = if path.is_empty() {
                        name.clone()
                    } else {
                        format!("{path}/{name}")
                    };
                    let at = self.rows.len();
                    self.rows.push(TreeRow::Folder {
                        path: path.clone(),
                        name,
                        depth,
                        end: 0,
                    });
                    self.draw(inside, &path, depth + 1);
                    let after = u32::try_from(self.rows.len()).unwrap_or(u32::MAX);
                    if let Some(TreeRow::Folder { end, .. }) = self.rows.get_mut(at) {
                        *end = after;
                    }
                }
            }
        }
    }
}

/// What a folder holds of `entries`, the files below it: each file, and each folder with the entries
/// inside it, in the order the diff first reaches them.
fn children<'p, 'a>(entries: Vec<Entry<'p, 'a>>) -> Vec<Child<'p, 'a>> {
    let mut children: Vec<Child<'_, '_>> = Vec::new();
    let mut folders: HashMap<&str, usize> = HashMap::new();
    for (file, parts) in entries {
        match parts {
            [] => {}
            [name] => children.push(Child::File { file, name }),
            [name, rest @ ..] => {
                let at = *folders.entry(name).or_insert_with(|| {
                    children.push(Child::Folder {
                        name,
                        inside: Vec::new(),
                    });
                    children.len() - 1
                });
                if let Some(Child::Folder { inside, .. }) = children.get_mut(at) {
                    inside.push((file, rest));
                }
            }
        }
    }
    children
}

/// The folder `name`, which holds `inside`, joined with the folder inside it while it holds only that one:
/// its name with theirs, and what the last of them holds.
fn joined<'p, 'a>(name: &str, mut inside: Vec<Entry<'p, 'a>>) -> (String, Vec<Entry<'p, 'a>>) {
    let mut name = name.to_owned();
    while let Some((_, [lone, _, ..])) = inside.first() {
        let lone = *lone;
        let only =
            (inside.iter()).all(|(_, parts)| matches!(parts, [folder, _, ..] if *folder == lone));
        if !only {
            break;
        }
        name = format!("{name}/{lone}");
        for (_, parts) in &mut inside {
            *parts = parts.get(1..).unwrap_or_default();
        }
    }
    (name, inside)
}

#[cfg(test)]
mod tests;
