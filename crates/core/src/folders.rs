//! Where Tsugiki keeps its files (ADR 0014). `find` takes the program's path and the environment, so tests
//! can check both OSes' layouts on either OS, and `current` passes it the real ones.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::{Error, ErrorKind};

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

/// The folders Tsugiki's files live in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folders {
    /// Holds `tsugiki.db`, `tsugiki.log`, `machine.toml` and `repos`.
    pub data: PathBuf,
    /// Holds `settings.toml`, unless `settings_path` in `machine.toml` says otherwise.
    pub settings: PathBuf,
}

impl Folders {
    /// Where `tsugiki.db` is.
    pub fn database(&self) -> PathBuf {
        self.data.join("tsugiki.db")
    }

    /// Where the log is. The file before it is `tsugiki.log.1`, next to it.
    pub fn log(&self) -> PathBuf {
        self.data.join("tsugiki.log")
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
        Self::find(Os::CURRENT, &exe, env)
    }

    /// Finds the portable folder next to the program if there is one, and the installed folders if not.
    pub fn find(os: Os, exe: &Path, env: &dyn Fn(&str) -> Option<OsString>) -> Result<Self, Error> {
        if let Some(portable) = portable_folder(os, exe) {
            return Ok(Self {
                settings: portable.clone(),
                data: portable,
            });
        }
        match os {
            Os::Windows => Ok(Self {
                data: folder_from_env(env, "LOCALAPPDATA")?.join("tsugiki"),
                settings: folder_from_env(env, "APPDATA")?.join("tsugiki"),
            }),
            Os::MacOs => {
                let data = folder_from_env(env, "HOME")?
                    .join("Library")
                    .join("Application Support")
                    .join("tsugiki");
                Ok(Self {
                    settings: data.clone(),
                    data,
                })
            }
        }
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
mod tests {
    use super::*;
    use std::fs;

    /// A new folder under `.temp`, with a path that is absolute on this OS.
    fn temp_folder(name: &str) -> PathBuf {
        let folder = crate::testing::temp_path(&format!("folders-{name}"));
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    /// An environment with the given variables, and no others.
    fn env_of(variables: &[(&str, &Path)]) -> impl Fn(&str) -> Option<OsString> + use<> {
        let variables: Vec<(String, OsString)> = variables
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.as_os_str().to_owned()))
            .collect();
        move |wanted| {
            variables
                .iter()
                .find(|(name, _)| name == wanted)
                .map(|(_, value)| value.clone())
        }
    }

    #[test]
    fn the_log_sits_beside_the_database() {
        let folders = Folders {
            data: PathBuf::from("data"),
            settings: PathBuf::from("settings"),
        };
        assert_eq!(folders.log(), Path::new("data").join("tsugiki.log"));
    }

    #[test]
    fn the_current_folders_come_from_this_program_and_its_real_environment() {
        let folders = Folders::current().unwrap();
        assert!(folders.database().is_absolute());
        assert!(folders.settings.is_absolute());
    }

    #[test]
    fn a_program_that_cant_tell_where_it_is_says_so() {
        let lost = Err(std::io::Error::from(std::io::ErrorKind::NotFound));

        let problem = Folders::from_exe(lost, &env_of(&[])).unwrap_err();

        assert_eq!(problem.kind, ErrorKind::Internal);
        assert_eq!(
            problem.message,
            "Tsugiki can't tell where it is running from."
        );
    }

    #[test]
    fn installed_on_windows_data_is_local_and_settings_roam() {
        let root = temp_folder("windows-installed");
        let (local, roaming) = (root.join("Local"), root.join("Roaming"));
        let env = env_of(&[("LOCALAPPDATA", &local), ("APPDATA", &roaming)]);
        let exe = local.join("Programs").join("Tsugiki").join("tsg.exe");

        let folders = Folders::find(Os::Windows, &exe, &env).unwrap();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(folders.data, local.join("tsugiki"));
        assert_eq!(folders.settings, roaming.join("tsugiki"));
    }

    #[test]
    fn installed_on_macos_everything_is_in_application_support() {
        let home = temp_folder("macos-installed");
        let env = env_of(&[("HOME", &home)]);
        let exe = home
            .join("Tsugiki.app")
            .join("Contents")
            .join("MacOS")
            .join("tsg");

        let folders = Folders::find(Os::MacOs, &exe, &env).unwrap();
        fs::remove_dir_all(&home).unwrap();

        let expected = home
            .join("Library")
            .join("Application Support")
            .join("tsugiki");
        assert_eq!(folders.data, expected);
        assert_eq!(folders.settings, expected);
    }

    #[test]
    fn a_data_folder_next_to_tsg_exe_makes_windows_portable() {
        let root = temp_folder("windows-portable");
        fs::create_dir(root.join("data")).unwrap();
        let no_variables = env_of(&[]);

        let folders = Folders::find(Os::Windows, &root.join("tsg.exe"), &no_variables).unwrap();
        fs::remove_dir_all(&root).unwrap();

        assert_eq!(folders.data, root.join("data"));
        assert_eq!(folders.settings, root.join("data"));
    }

    #[test]
    fn a_folder_next_to_the_app_bundle_makes_macos_portable() {
        let root = temp_folder("macos-portable");
        let portable = root.join("tsugiki-portable-data");
        fs::create_dir(&portable).unwrap();
        let exe = root
            .join("Tsugiki.app")
            .join("Contents")
            .join("MacOS")
            .join("tsg");

        let folders = Folders::find(Os::MacOs, &exe, &env_of(&[])).unwrap();
        fs::remove_dir_all(&root).unwrap();

        assert_eq!(folders.data, portable);
        assert_eq!(folders.settings, portable);
    }

    #[test]
    fn a_macos_program_outside_a_bundle_looks_next_to_itself() {
        let root = temp_folder("macos-bare");
        let portable = root.join("tsugiki-portable-data");
        fs::create_dir(&portable).unwrap();

        let folders = Folders::find(Os::MacOs, &root.join("tsg"), &env_of(&[])).unwrap();
        fs::remove_dir_all(&root).unwrap();

        assert_eq!(folders.data, portable);
    }

    #[test]
    fn a_file_named_data_doesnt_make_tsugiki_portable() {
        let root = temp_folder("file-named-data");
        fs::write(root.join("data"), "").unwrap();
        let env = env_of(&[("LOCALAPPDATA", &root), ("APPDATA", &root)]);

        let folders = Folders::find(Os::Windows, &root.join("tsg.exe"), &env).unwrap();
        fs::remove_dir_all(&root).unwrap();

        assert_eq!(folders.data, root.join("tsugiki"));
    }

    #[test]
    fn each_os_looks_only_for_its_own_portable_folder() {
        let root = temp_folder("other-os-portable");
        let env = env_of(&[("HOME", &root), ("LOCALAPPDATA", &root), ("APPDATA", &root)]);

        fs::create_dir(root.join("data")).unwrap();
        let macos = Folders::find(Os::MacOs, &root.join("tsg"), &env).unwrap();
        fs::remove_dir(root.join("data")).unwrap();

        fs::create_dir(root.join("tsugiki-portable-data")).unwrap();
        let windows = Folders::find(Os::Windows, &root.join("tsg.exe"), &env).unwrap();
        fs::remove_dir_all(&root).unwrap();

        assert_eq!(
            macos.data,
            root.join("Library")
                .join("Application Support")
                .join("tsugiki")
        );
        assert_eq!(windows.data, root.join("tsugiki"));
    }

    #[test]
    fn a_missing_variable_is_an_error_that_names_it() {
        let root = temp_folder("missing");
        let only_local = env_of(&[("LOCALAPPDATA", &root)]);
        let exe = root.join("tsg.exe");

        let windows = Folders::find(Os::Windows, &exe, &only_local).unwrap_err();
        let macos = Folders::find(Os::MacOs, &exe, &only_local).unwrap_err();
        fs::remove_dir_all(&root).unwrap();

        assert_eq!(windows.kind, ErrorKind::Internal);
        assert!(windows.message.contains(" APPDATA "), "{}", windows.message);
        assert!(macos.message.contains(" HOME "), "{}", macos.message);
    }

    #[test]
    fn an_empty_or_relative_variable_counts_as_missing() {
        // A relative path would put the database wherever Tsugiki happened to start.
        let root = temp_folder("relative");
        let exe = root.join("tsg");
        let relative = env_of(&[("HOME", Path::new("relative"))]);
        let empty = env_of(&[("HOME", Path::new(""))]);

        let relative = Folders::find(Os::MacOs, &exe, &relative).unwrap_err();
        let empty = Folders::find(Os::MacOs, &exe, &empty).unwrap_err();
        fs::remove_dir_all(&root).unwrap();

        assert!(relative.message.contains("HOME"), "{}", relative.message);
        assert!(empty.message.contains("HOME"), "{}", empty.message);
    }
}
