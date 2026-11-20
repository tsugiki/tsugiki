//! The session's read-only copy of the settings of the Repositories it works in. The settings owner keeps
//! the settings, and hands the session each save.

use std::mem;

use crate::repository::RepositoryName;
use crate::repository_settings::RepositorySettings;

/// The settings of each Repository opened since launch, as the session last heard them, with or without a
/// token: the one copy every sync and every move of Reach a mark makes reads them from.
#[derive(Default)]
pub struct HeldSettings {
    /// The settings of each Repository opened since launch.
    opened: Vec<RepositorySettings>,
    /// The settings the settings owner last handed over of each Repository, which it hands over in the
    /// order it saves them. Opening a Repository takes these over the copy the command that opened it read,
    /// which a save handed over meanwhile can have outdated.
    owned: Vec<RepositorySettings>,
}

impl HeldSettings {
    /// Holds `repository`'s settings as opened, unless the settings owner handed over its own: those are as
    /// new as any, and the copy the command read can be older. Answers with those it holds now.
    pub fn open(&mut self, repository: RepositorySettings) -> &RepositorySettings {
        let owned = (self.owned.iter()).find(|owned| owned.name.is_same(&repository.name));
        let repository = owned.cloned().unwrap_or(repository);
        self.opened
            .retain(|held| !held.name.is_same(&repository.name));
        self.opened.push_mut(repository)
    }

    /// Takes the settings the settings owner handed over, `repository`'s, in place of those held, if it was
    /// opened since launch and they differ, and answers with the settings it replaced.
    pub fn replace(&mut self, repository: RepositorySettings) -> Option<RepositorySettings> {
        let owned = (self.owned.iter_mut()).find(|owned| owned.name.is_same(&repository.name));
        match owned {
            Some(owned) => owned.clone_from(&repository),
            None => self.owned.push(repository.clone()),
        }
        let held = (self.opened.iter_mut()).find(|held| held.name.is_same(&repository.name))?;
        if *held == repository {
            return None;
        }
        Some(mem::replace(held, repository))
    }

    /// The settings of `repository`, if it was opened since launch.
    pub fn get(&self, repository: &RepositoryName) -> Option<&RepositorySettings> {
        (self.opened.iter()).find(|settings| settings.name.is_same(repository))
    }
}
