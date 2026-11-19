//! The store's read-only connection and the view models' reads on it, for the benchmarks that time them.
//! Outside the core only [`crate::calls::Calls`] makes these reads, and a benchmark has no session to make
//! one from.

use std::path::Path;

use time::OffsetDateTime;

use crate::error::Error;
use crate::facts::WorkFacts;
use crate::repository::RepositoryName;
use crate::repository_settings::RepositorySettings;
use crate::settings::ListOrder;
use crate::store::{self, OpenReader, StoreReader};
use crate::workbench::inbox::{Inbox, InboxPage};
use crate::workbench::issue_view::IssueView;
use crate::workbench::pr_view::PrView;
use crate::workbench::quick_open::{self, QuickOpenFound};
use crate::workbench::{Workbench, WorkbenchView};

/// A read-only connection to a database, and the reads the window's screens make on it.
pub struct Reads {
    reader: StoreReader,
    open: OpenReader,
}

impl Reads {
    /// Opens the database at `path` read-only, once `Store::open` has made it current.
    pub fn open(path: &Path) -> Result<Self, Error> {
        let (to_open, reader) = store::reader();
        let open = to_open.open(path)?;
        Ok(Self { reader, open })
    }

    /// The facts behind the Workbench of `repository`.
    pub fn work_facts(&self, repository: &RepositoryName) -> Result<WorkFacts, Error> {
        self.reader.work_facts(repository)
    }

    pub fn workbench(
        &self,
        settings: &RepositorySettings,
        view: &WorkbenchView,
        order: ListOrder,
    ) -> Result<Workbench, Error> {
        Workbench::read(&self.reader, settings, view, order)
    }

    pub fn inbox(
        &self,
        settings: &RepositorySettings,
        page: InboxPage,
        now: OffsetDateTime,
    ) -> Result<Inbox, Error> {
        Inbox::read(&self.reader, settings, page, now)
    }

    pub fn issue_view(
        &self,
        settings: &RepositorySettings,
        number: u32,
    ) -> Result<Option<IssueView>, Error> {
        IssueView::read(&self.reader, settings, number)
    }

    pub fn pr_view(
        &self,
        settings: &RepositorySettings,
        number: u32,
    ) -> Result<Option<PrView>, Error> {
        PrView::read(&self.reader, settings, number)
    }

    pub fn quick_open(
        &self,
        repository: &RepositoryName,
        typed: &str,
    ) -> Result<QuickOpenFound, Error> {
        quick_open::find(&self.reader, repository, typed)
    }

    /// Closes the connection. Close this before the `Store`: the last connection to close removes the
    /// `-wal` and `-shm` files.
    pub fn close(self) {
        self.open.close();
    }
}

#[cfg(test)]
mod tests;
