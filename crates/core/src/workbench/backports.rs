//! Planting each Backport under its Origin PR, so a tree stays two levels (ADR 0011).

use std::collections::{BTreeMap, BTreeSet};

use super::links::Linked;
use super::{BackportRow, PrTree, TreeEntry, WorkbenchItem, WorkbenchRow};
use crate::links::{BackportKind, BackportLink};

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

/// The PRs a Backport can sit under, of those each with its Backport link: the ones that aren't Backports
/// too, which keeps a tree two levels deep. A Bot backport shares the review state of the one it sits under.
pub(super) fn origins<'a>(
    pull_requests: impl Iterator<Item = (u32, Option<&'a BackportLink>)>,
) -> BTreeSet<u32> {
    let origins = pull_requests.filter(|(_, backport)| backport.is_none());
    origins.map(|(number, _)| number).collect()
}

/// Puts each Backport beneath its Origin PR when Tsugiki has that PR and it isn't a Backport too. The order
/// of `linked` stays within each level.
pub(super) fn plant(linked: Vec<Linked>) -> Vec<Top> {
    let origins = origins(
        linked
            .iter()
            .map(|pr| (pr.row.number, pr.backport.as_ref())),
    );
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
mod tests;
