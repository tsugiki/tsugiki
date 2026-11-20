//! Syncs whose settings a test lists, as the settings owner's view would.

use super::Syncs;
use crate::latest;
use crate::repository_settings::RepositorySettings;
use crate::settings::SettingsView;

impl Default for Syncs {
    /// Syncs reading a view that lists no Repository yet.
    fn default() -> Self {
        Self::new(latest::channel(SettingsView::default()).1)
    }
}

impl Syncs {
    /// Has the view these syncs read list `repository` with these settings, in place of any it listed by
    /// that name, as the settings owner publishes a save.
    pub(crate) fn list(&mut self, repository: RepositorySettings) {
        let mut view = self.settings.get();
        let listed = &mut view.loaded.settings.repositories;
        listed.retain(|held| !held.name.is_same(&repository.name));
        listed.push(repository);
        self.settings = latest::channel(view).1;
    }

    /// Lists `repository` with these settings, then opens it.
    pub(crate) fn add_listed(&mut self, repository: RepositorySettings) {
        let name = repository.name.clone();
        self.list(repository);
        self.add(name);
    }
}
