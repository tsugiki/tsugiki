//! Planting each Backport under its Origin PR, so a tree stays two levels (ADR 0011).

use std::collections::{BTreeMap, BTreeSet};

use super::links::Linked;
use super::{BackportRow, PrTree, TreeEntry, WorkbenchItem, WorkbenchRow};
use crate::links::BackportKind;
use crate::work::origins;

/// A Backport beneath its Origin PR.
struct Beneath<R, K> {
    row: R,
    kind: K,
    issues: BTreeSet<u32>,
}

/// The two shapes of a tree's top line.
enum Shape<R, K> {
    Pr {
        beneath: Vec<Beneath<R, K>>,
    },
    /// A Backport with no Origin PR to sit under.
    Backport(K),
}

/// A line that sits directly under an Issue or on its own. `R` is what draws a line, and `K` a Backport's
/// kind, which a grouping that only places PRs leaves as `()`.
pub(super) struct Top<R, K> {
    pub(super) number: u32,
    row: R,
    shape: Shape<R, K>,
    /// Each Issue it shows under: its own, and those of the Backports beneath it (ADR 0011).
    pub(super) issues: BTreeSet<u32>,
}

impl<R, K> Top<R, K> {
    fn beneath(&self) -> &[Beneath<R, K>] {
        match &self.shape {
            Shape::Backport(_) => &[],
            Shape::Pr { beneath } => beneath,
        }
    }

    /// The Backports beneath it that show under `issue`: those that fix it, and those that fix no Issue and
    /// follow their Origin PR.
    fn beneath_under(&self, issue: u32) -> impl Iterator<Item = &Beneath<R, K>> {
        (self.beneath().iter())
            .filter(move |below| below.issues.is_empty() || below.issues.contains(&issue))
    }

    /// Each line it shows under `issue`: its own, then the Backports beneath it there.
    pub(super) fn rows_under(&self, issue: u32) -> Vec<&R> {
        let beneath = self.beneath_under(issue).map(|below| &below.row);
        std::iter::once(&self.row).chain(beneath).collect()
    }

    /// Each line it shows on its own, with no Issue: its own, then every Backport beneath it.
    pub(super) fn rows(&self) -> Vec<&R> {
        let beneath = self.beneath().iter().map(|below| &below.row);
        std::iter::once(&self.row).chain(beneath).collect()
    }
}

impl Top<WorkbenchRow, BackportKind> {
    /// Its entry under `issue`, with the Backports that show there.
    pub(super) fn entry_under(&self, issue: u32) -> TreeEntry {
        let row = self.row.clone();
        match &self.shape {
            Shape::Backport(backport) => TreeEntry::Backport(BackportRow {
                row,
                backport: *backport,
            }),
            Shape::Pr { .. } => TreeEntry::Pr(PrTree {
                row,
                backports: (self.beneath_under(issue))
                    .map(|below| BackportRow {
                        row: below.row.clone(),
                        backport: below.kind,
                    })
                    .collect(),
            }),
        }
    }
}

/// Puts each Backport beneath its Origin PR when Tsugiki has that PR and it isn't a Backport too. The order
/// of `linked` stays within each level.
pub(super) fn plant<R, K>(linked: Vec<Linked<R, K>>) -> Vec<Top<R, K>> {
    let origins = origins(linked.iter().map(|pr| (pr.number, pr.backport.as_ref())));
    let mut beneath: BTreeMap<u32, Vec<Beneath<R, K>>> = BTreeMap::new();
    let mut tops = Vec::new();
    for pr in linked {
        match pr.backport {
            Some(link) if origins.contains(&link.origin) => {
                beneath.entry(link.origin).or_default().push(Beneath {
                    row: pr.row,
                    kind: link.kind,
                    issues: pr.issues,
                });
            }
            backport => tops.push(Top {
                number: pr.number,
                row: pr.row,
                shape: backport.map_or_else(
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
            && let Some(below) = beneath.remove(&top.number)
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
pub(super) fn standalone(top: Top<WorkbenchRow, BackportKind>) -> WorkbenchItem {
    match top.shape {
        Shape::Backport(backport) => WorkbenchItem::StandaloneBackport(BackportRow {
            row: top.row,
            backport,
        }),
        Shape::Pr { beneath } => WorkbenchItem::StandalonePr(PrTree {
            row: top.row,
            backports: beneath
                .into_iter()
                .map(|below| BackportRow {
                    row: below.row,
                    backport: below.kind,
                })
                .collect(),
        }),
    }
}

#[cfg(test)]
mod tests;
