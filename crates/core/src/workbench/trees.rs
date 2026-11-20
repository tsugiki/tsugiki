//! The trees of a few Issues and PRs, read through the links the store keeps (#833), for a view that shows
//! only those: an Inbox page, the Issue view and the PR view. Each read takes the rows it names, so its cost
//! follows the trees and not the store's size.

use std::collections::{BTreeMap, BTreeSet};

use super::backports::plant;
use super::links::placed;
use crate::conventions::Conventions;
use crate::error::Error;
use crate::facts::WorkFacts;
use crate::facts::pull_requests::PrLinks;
use crate::login::Login;
use crate::repository_settings::RepositorySettings;
use crate::store::{Snapshot, StoreReader};

/// What the view of one Issue or PR reads in one snapshot: the facts of what its trees hold, the links read
/// of their PRs, and what else the view shows of the item, such as what it says.
pub(super) struct ItemRead<M> {
    pub(super) facts: WorkFacts,
    pub(super) links: BTreeMap<u32, PrLinks>,
    pub(super) more: M,
}

impl<M: Send + 'static> ItemRead<M> {
    /// Reads, in one snapshot of `repository`, the Issues and PRs that `trees_of` reads the trees of and
    /// names, then what `more` reads. Nothing before anyone signs in, when no view shows anything. This
    /// waits on the database and never on GitHub (ADR 0018).
    pub(super) fn read(
        reader: &StoreReader,
        repository: &RepositorySettings,
        trees_of: impl FnOnce(&mut Trees<'_, '_>) -> Result<Named, Error> + Send + 'static,
        more: impl FnOnce(&Snapshot<'_>) -> Result<M, Error> + Send + 'static,
    ) -> Result<Option<Self>, Error> {
        let conventions = repository.conventions;
        reader.read_snapshot(&repository.name, move |snapshot| {
            let Some(viewer) = snapshot.viewer()? else {
                return Ok(None);
            };
            let mut trees = Trees::new(snapshot, conventions);
            let named = trees_of(&mut trees)?;
            let facts = trees.facts(viewer, &named.issues, &named.prs)?;
            Ok(Some(Self {
                facts,
                links: trees.into_links(),
                more: more(snapshot)?,
            }))
        })
    }
}

/// The Issues and PRs whose facts a read takes.
pub(super) struct Named {
    pub(super) issues: BTreeSet<u32>,
    pub(super) prs: BTreeSet<u32>,
}

/// The links read so far in one snapshot, which each read of a tree adds to.
pub(super) struct Trees<'s, 't> {
    snapshot: &'s Snapshot<'t>,
    conventions: Conventions,
    links: BTreeMap<u32, PrLinks>,
    /// Each PR whose links were asked for, so a PR the store lacks is asked for once.
    asked: BTreeSet<u32>,
}

impl<'s, 't> Trees<'s, 't> {
    pub(super) fn new(snapshot: &'s Snapshot<'t>, conventions: Conventions) -> Self {
        Self {
            snapshot,
            conventions,
            links: BTreeMap::new(),
            asked: BTreeSet::new(),
        }
    }

    /// Reads the links of `prs`, of their Origin PRs, and of every PR that names one of either as its
    /// Origin PR: all that says where each of `prs` sits, and what sits with it. Gives the numbers of all
    /// those PRs, which `plant` puts on their lines as it would among every PR.
    pub(super) fn of_prs(&mut self, prs: &BTreeSet<u32>) -> Result<BTreeSet<u32>, Error> {
        if prs.is_empty() {
            return Ok(BTreeSet::new());
        }
        self.read_links(prs)?;
        let origins: BTreeSet<u32> = (prs.iter())
            .filter_map(|number| self.links.get(number)?.origin)
            .collect();
        self.read_links(&origins)?;
        let mut trees: BTreeSet<u32> = prs.union(&origins).copied().collect();
        let backports = self.snapshot.prs_naming_origins(&trees, self.conventions)?;
        self.read_links(&backports)?;
        trees.extend(backports);
        Ok(trees)
    }

    /// Reads the trees of the PRs that fix one of `issues`, as `of_prs` does, and gives their numbers.
    pub(super) fn under(&mut self, issues: &BTreeSet<u32>) -> Result<BTreeSet<u32>, Error> {
        if issues.is_empty() {
            return Ok(BTreeSet::new());
        }
        let fixing = self.snapshot.prs_fixing(issues, self.conventions)?;
        self.of_prs(&fixing)
    }

    /// The Issues that the line holding PR `number` shows under, among the lines of `prs`, whose trees
    /// `of_prs` read.
    pub(super) fn issues_holding(&self, prs: &BTreeSet<u32>, number: u32) -> BTreeSet<u32> {
        let tops = plant(placed(self.links_of(prs))).into_iter();
        tops.filter(|top| top.rows().iter().any(|links| links.number == number))
            .flat_map(|top| top.issues)
            .collect()
    }

    fn read_links(&mut self, numbers: &BTreeSet<u32>) -> Result<(), Error> {
        let unasked: BTreeSet<u32> = numbers.difference(&self.asked).copied().collect();
        if unasked.is_empty() {
            return Ok(());
        }
        self.asked.extend(&unasked);
        let read = self.snapshot.pr_links(&unasked, self.conventions)?;
        self.links
            .extend(read.into_iter().map(|links| (links.number, links)));
        Ok(())
    }

    /// The links read of each of `numbers` that the store holds.
    pub(super) fn links_of<'a>(
        &'a self,
        numbers: &'a BTreeSet<u32>,
    ) -> impl Iterator<Item = &'a PrLinks> {
        numbers.iter().filter_map(|number| self.links.get(number))
    }

    /// The whole facts of `issues` and `prs` alone, with their activity and their Reviews, as `viewer`
    /// reads them. They hold no Filter's results.
    pub(super) fn facts(
        &self,
        viewer: Login,
        issues: &BTreeSet<u32>,
        prs: &BTreeSet<u32>,
    ) -> Result<WorkFacts, Error> {
        let pull_requests = self.snapshot.pull_requests_numbered(prs)?;
        let numbers: BTreeSet<u32> = issues.union(prs).copied().collect();
        Ok(WorkFacts {
            activity: self.snapshot.activity_of(Some(&viewer), &numbers)?,
            reviews: self.snapshot.reviews_of(&pull_requests)?,
            viewer: Some(viewer),
            issues: self.snapshot.issues_numbered(issues)?,
            pull_requests,
            filters: BTreeMap::new(),
        })
    }

    /// The links read, by each PR's number.
    pub(super) fn into_links(self) -> BTreeMap<u32, PrLinks> {
        self.links
    }
}
