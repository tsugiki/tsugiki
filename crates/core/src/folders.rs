//! Where Tsugiki keeps its files (ADR 0014). `find` takes the program's path and the environment, so tests
//! can check both OSes' layouts on either OS, and `current` passes it the real ones.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use crate::build::Build;
use crate::error::{Error, ErrorKind};
use crate::repository::RepositoryName;

/// The OS whose layout applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Windows,
    MacOs,
}

impl Os {
    /// The OS this build is for.
    #[cfg(windows)]
    pub const CURRENT: Self = Self::Windows;
    /// The OS this build is for. Linux isn't a v1 target (ADR 0001), and only builds the core for its
    /// benchmarks (ADR 0017), so it takes macOS's layout rather than a third one nothing would use.
    #[cfg(not(windows))]
    pub const CURRENT: Self = Self::MacOs;
}

/// A **Mirror**'s folder, in its Repository's folder.
const MIRROR: &str = "mirror.git";
/// The **Lookup checkout**'s folder, in its Repository's folder.
const LOOKUP: &str = "lookup";
/// The folder of the files Tsugiki and the language servers keep for a Repository, in its folder.
const SERVERS: &str = "servers";

/// The folder of the files the language servers keep for the Repository whose **Lookup checkout** is at
/// `lookup`: `servers` beside it.
pub fn servers_beside(lookup: &Path) -> PathBuf {
    lookup.with_file_name(SERVERS)
}

/// The root of the repo this program was built from, two folders up from this crate's, where `just dev` runs.
/// None once the checkout has moved or gone, or on a machine that never had it.
fn repo_root() -> Option<PathBuf> {
    checkout_root(Path::new(env!("CARGO_MANIFEST_DIR")))
}

/// The checkout two folders up from a crate's folder, if it still holds the `justfile`.
fn checkout_root(crate_folder: &Path) -> Option<PathBuf> {
    let root = crate_folder.ancestors().nth(2)?;
    root.join("justfile").is_file().then(|| root.to_owned())
}

/// The folders Tsugiki's files live in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folders {
    /// Holds `tsugiki.db`, `machine.toml`, `repos` and `webview`.
    pub data: PathBuf,
    /// Holds `settings.toml`, unless `settings_path` in `machine.toml` says otherwise.
    pub settings: PathBuf,
    /// Holds `tsugiki.log` and `tsugiki.log.1`: `data`, or the repo's root for a development build that
    /// isn't portable and still has its checkout.
    pub logs: PathBuf,
}

impl Folders {
    /// Where `tsugiki.db` is.
    pub fn database(&self) -> PathBuf {
        self.data.join("tsugiki.db")
    }

    /// Where the log is. The file before it is `tsugiki.log.1`, next to it.
    pub fn log(&self) -> PathBuf {
        self.logs.join("tsugiki.log")
    }

    /// Where the webview on Windows keeps its profile and cache, so a portable copy stays in its folder.
    pub fn webview(&self) -> PathBuf {
        self.data.join("webview")
    }

    /// Where `machine.toml` is.
    pub fn machine_file(&self) -> PathBuf {
        self.data.join("machine.toml")
    }

    /// A Repository's folder under `repos` (ADR 0010). GitHub's names ignore case, so the folder's name
    /// doesn't keep it.
    pub fn repository(&self, repository: &RepositoryName) -> PathBuf {
        let folder = format!("{}__{}", repository.owner(), repository.name()).to_lowercase();
        self.data.join("repos").join(folder)
    }

    /// Where a Repository's **Mirror** is, in its folder.
    pub fn mirror(&self, repository: &RepositoryName) -> PathBuf {
        self.repository(repository).join(MIRROR)
    }

    /// Where a Repository's **Lookup checkout** is, beside its Mirror.
    pub fn lookup(&self, repository: &RepositoryName) -> PathBuf {
        self.repository(repository).join(LOOKUP)
    }

    /// Where `settings.toml` is, unless `settings_path` in `machine.toml` says otherwise.
    pub fn settings_file(&self) -> PathBuf {
        self.settings.join("settings.toml")
    }

    /// The folders for this program, from its real path and environment.
    pub fn current() -> Result<Self, Error> {
        Self::from_exe(std::env::current_exe(), &|variable| {
            std::env::var_os(variable)
        })
    }

    fn from_exe(
        exe: io::Result<PathBuf>,
        env: &dyn Fn(&str) -> Option<OsString>,
    ) -> Result<Self, Error> {
        #[expect(
            clippy::map_err_ignore,
            reason = "the log lives in the folder this is looking for, so it has nowhere to go"
        )]
        let exe = exe.map_err(|_| {
            Error::new(
                ErrorKind::Internal,
                "Tsugiki can't tell where it is running from.",
            )
        })?;
        Self::find(Os::CURRENT, Build::CURRENT, &exe, env)
    }

    /// Finds the portable folder next to the program if there is one, and the installed folders if not.
    pub fn find(
        os: Os,
        build: Build,
        exe: &Path,
        env: &dyn Fn(&str) -> Option<OsString>,
    ) -> Result<Self, Error> {
        if let Some(portable) = portable_folder(os, exe) {
            return Ok(Self {
                settings: portable.clone(),
                logs: portable.clone(),
                data: portable,
            });
        }
        let name = build.folder_name();
        let (data, settings) = match os {
            Os::Windows => (
                folder_from_env(env, "LOCALAPPDATA")?.join(name),
                folder_from_env(env, "APPDATA")?.join(name),
            ),
            Os::MacOs => {
                let data = folder_from_env(env, "HOME")?
                    .join("Library")
                    .join("Application Support")
                    .join(name);
                (data.clone(), data)
            }
        };
        // A developer finds the log in the folder they ran `just dev` from.
        let logs = match build {
            Build::Release => None,
            Build::Development => repo_root(),
        };
        let logs = logs.unwrap_or_else(|| data.clone());
        Ok(Self {
            data,
            settings,
            logs,
        })
    }
}

/// The portable folder next to the program, or next to the bundle that holds it on macOS, if it exists.
fn portable_folder(os: Os, exe: &Path) -> Option<PathBuf> {
    let (program, name) = match os {
        Os::Windows => (exe, "data"),
        Os::MacOs => {
            let is_bundle = |path: &&Path| path.extension().is_some_and(|ext| ext == "app");
            let bundle = exe.ancestors().find(is_bundle);
            (bundle.unwrap_or(exe), "tsugiki-portable-data")
        }
    };
    let folder = program.parent()?.join(name);
    folder.is_dir().then_some(folder)
}

/// A relative path would put Tsugiki's files wherever it happened to start, so it counts as unset.
fn folder_from_env(
    env: &dyn Fn(&str) -> Option<OsString>,
    variable: &str,
) -> Result<PathBuf, Error> {
    env(variable)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| {
            let message =
                format!("Tsugiki can't find its folders, because {variable} isn't a full path.");
            Error::new(ErrorKind::Internal, &message)
        })
}

#[cfg(test)]
mod tests;
