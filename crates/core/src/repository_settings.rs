//! A Repository the user added, with its Conventions and Filters, and what adding one would take (ADR 0011,
//! ADR 0015).

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::conventions::Conventions;
use crate::filters::Filter;
use crate::repository::{
    FoundRepository, RepositoryAccess, RepositoryName, RepositorySize, WINDOW_DAYS,
};

/// Measured on CPython's last 30 days (#132): 4,760 items took about 485 points and 20.5 minutes, one request
/// at a time, with whole trees for the user's own Issues. About 10 of those points are the user's own work
/// and don't grow with the window.
const FIXED_POINTS: u32 = 10;
const POINTS_PER_THOUSAND_ITEMS: u32 = 100;
const MILLISECONDS_PER_ITEM: u32 = 260;

/// Measured on CPython (#13), whose size on GitHub is about 913,000 KB: a plain clone took about 3.5
/// minutes, and one that borrowed objects from a clone on the same disk about 36 s.
const DOWNLOAD_KB_PER_SECOND: u64 = 4_350;
const BORROW_KB_PER_SECOND: u64 = 25_000;
const KB_PER_MB: u64 = 1024;

/// A Repository the user added, as `settings.toml` holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(from = "RepositoryInFile")]
pub struct RepositorySettings {
    pub name: RepositoryName,
    pub conventions: Conventions,
    pub filters: Vec<Filter>,
}

impl RepositorySettings {
    /// The settings adding `name` gives it, for a user with this `access`.
    pub fn added(name: RepositoryName, access: RepositoryAccess) -> Self {
        let conventions = Conventions::for_repository(&name);
        Self {
            name,
            conventions,
            filters: Filter::starters(access),
        }
    }

    /// A Repository Tsugiki knows nothing else about.
    pub fn new(name: RepositoryName) -> Self {
        Self::added(name, RepositoryAccess::Other)
    }
}

/// A Repository added by hand may leave its Conventions and its Filters out. Then it gets what adding it
/// would give it, as far as the file can say: only GitHub knows if the user administers it. `filters = []`
/// means no Filters.
#[derive(Deserialize, Type)]
struct RepositoryInFile {
    name: RepositoryName,
    conventions: Option<Conventions>,
    filters: Option<Vec<Filter>>,
}

impl From<RepositoryInFile> for RepositorySettings {
    fn from(file: RepositoryInFile) -> Self {
        let conventions = file
            .conventions
            .unwrap_or_else(|| Conventions::for_repository(&file.name));
        let filters = file
            .filters
            .unwrap_or_else(|| Filter::starters(RepositoryAccess::Other));
        Self {
            name: file.name,
            conventions,
            filters,
        }
    }
}

/// What the add dialog shows before the user decides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct RepositoryCheck {
    pub repository: RepositorySettings,
    pub access: RepositoryAccess,
    pub open_issues: u32,
    pub open_prs: u32,
    pub size: RepositorySize,
    pub window_days: u8,
    /// How many Issues and PRs the first sync loads.
    pub first_sync_items: u32,
    pub first_sync_seconds: u32,
    pub first_sync_points: u32,
    pub mirror: MirrorCost,
}

/// What a Repository's mirror takes, for the user to choose where it gets its objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub struct MirrorCost {
    /// The size of the mirror on the disk, whether it borrows objects or not: `--dissociate` copies them.
    pub mirror_mb: u32,
    /// About how long its clone takes from GitHub alone, and borrowing from a clone the user has.
    pub download_seconds: u32,
    pub borrow_seconds: u32,
}

impl MirrorCost {
    /// The cost of the mirror of a Repository that takes `disk_kb` on GitHub.
    fn new(disk_kb: u64) -> Self {
        Self {
            mirror_mb: clamped(disk_kb / KB_PER_MB),
            download_seconds: clamped(disk_kb / DOWNLOAD_KB_PER_SECOND),
            borrow_seconds: clamped(disk_kb / BORROW_KB_PER_SECOND),
        }
    }
}

impl RepositoryCheck {
    /// What adding the Repository GitHub `found` would give and cost.
    pub fn new(found: FoundRepository) -> Self {
        let FoundRepository {
            name,
            counts,
            access,
        } = found;
        let size = counts.size();
        let items = match size {
            RepositorySize::Small => counts.open().saturating_add(counts.closed_in_window),
            RepositorySize::Large => counts.updated_in_window,
        };
        Self {
            repository: RepositorySettings::added(name, access),
            access,
            open_issues: counts.open_issues,
            open_prs: counts.open_prs,
            size,
            window_days: WINDOW_DAYS,
            first_sync_items: items,
            first_sync_seconds: items.saturating_mul(MILLISECONDS_PER_ITEM) / 1000,
            first_sync_points: FIXED_POINTS
                .saturating_add(items.saturating_mul(POINTS_PER_THOUSAND_ITEMS) / 1000),
            mirror: MirrorCost::new(counts.disk_kb),
        }
    }
}

/// What Add repository learns about the name the user typed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NameCheck {
    /// Tsugiki has this Repository already, by this name, so the dialog offers to switch to it.
    Held { name: RepositoryName },
    /// What adding it would give and cost.
    New { check: RepositoryCheck },
}

/// `value`, or the largest `u32` for more.
fn clamped(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests;
