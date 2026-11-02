//! Where Tsugiki keeps its files (ADR 0014). The program's path and the environment are passed in, so
//! tests can check both OSes' layouts on either OS.

use std::ffi::OsString;
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
    /// Holds `tsugiki.db`, `machine.toml` and `repos`.
    pub data: PathBuf,
    /// Holds `settings.toml`, unless `settings_path` in `machine.toml` says otherwise.
    pub settings: PathBuf,
}

impl Folders {
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
                data: folder_from_env(env, "LOCALAPPDATA")?.join("Tsugiki"),
                settings: folder_from_env(env, "APPDATA")?.join("Tsugiki"),
            }),
            Os::MacOs => {
                let data = folder_from_env(env, "HOME")?
                    .join("Library")
                    .join("Application Support")
                    .join("Tsugiki");
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
            (bundle.unwrap_or(exe), "Tsugiki-portable-data")
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

    /// A new folder under the system's temp folder, with a path that is absolute on this OS.
    fn temp_folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("tsugiki-folders-{name}-{}", std::process::id()));
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
    fn installed_on_windows_data_is_local_and_settings_roam() {
        let root = temp_folder("windows-installed");
        let (local, roaming) = (root.join("Local"), root.join("Roaming"));
        let env = env_of(&[("LOCALAPPDATA", &local), ("APPDATA", &roaming)]);
        let exe = local.join("Programs").join("Tsugiki").join("tsg.exe");

        let folders = Folders::find(Os::Windows, &exe, &env).unwrap();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(folders.data, local.join("Tsugiki"));
        assert_eq!(folders.settings, roaming.join("Tsugiki"));
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
            .join("Tsugiki");
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
        let portable = root.join("Tsugiki-portable-data");
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
        let portable = root.join("Tsugiki-portable-data");
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

        assert_eq!(folders.data, root.join("Tsugiki"));
    }

    #[test]
    fn each_os_looks_only_for_its_own_portable_folder() {
        let root = temp_folder("other-os-portable");
        let env = env_of(&[("HOME", &root), ("LOCALAPPDATA", &root), ("APPDATA", &root)]);

        fs::create_dir(root.join("data")).unwrap();
        let macos = Folders::find(Os::MacOs, &root.join("tsg"), &env).unwrap();
        fs::remove_dir(root.join("data")).unwrap();

        fs::create_dir(root.join("Tsugiki-portable-data")).unwrap();
        let windows = Folders::find(Os::Windows, &root.join("tsg.exe"), &env).unwrap();
        fs::remove_dir_all(&root).unwrap();

        assert_eq!(
            macos.data,
            root.join("Library")
                .join("Application Support")
                .join("Tsugiki")
        );
        assert_eq!(windows.data, root.join("Tsugiki"));
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
