//! What the user configures, in `settings.toml`, which travels, and `machine.toml`, which stays (ADR 0014).
//!
//! Tsugiki reads both when it starts and when the Settings window opens, and an edit made in between waits
//! for one of those. That window's Apply saves what changed in it. A file that can't be used stays as it is,
//! `problems` says why, and the last good values stay.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::thread;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use specta::Type;
use toml_edit::{ArrayOfTables, DocumentMut, Item, Key, RawString, Table, TableLike, Value};

use crate::error::{Error, ErrorKind};
use crate::folders::Folders;
use crate::latest::Latest;
use crate::repository::{Home, RepositoryName, RepositorySettings};

const REPOSITORIES: &str = "repositories";
const LAST_REPOSITORY: &str = "last_repository";
const STOPPED_SAVING: &str =
    "Tsugiki stopped saving its settings. Restart Tsugiki to save them again.";

/// What `settings.toml` holds. A value the file leaves out takes its default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct Settings {
    pub status_bar: StatusBarSettings,
    pub repositories: Vec<RepositorySettings>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct StatusBarSettings {
    /// Being offline or a problem still shows when this is off, since nothing else would say so.
    pub visible: bool,
}

impl Default for StatusBarSettings {
    fn default() -> Self {
        Self { visible: true }
    }
}

/// The settings in force, and what is wrong with the files they came from, in words for the status bar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Type)]
pub struct LoadedSettings {
    pub settings: Settings,
    pub problems: Vec<String>,
}

/// What the rest of Tsugiki may hold a copy of, so it can answer without waiting on the files or their owner.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsView {
    pub loaded: LoadedSettings,
    pub home: Home,
}

/// What `machine.toml` holds, as far as this file needs it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct Machine {
    /// Where `settings.toml` is, if not in its usual folder. A relative path starts at `machine.toml`'s
    /// folder.
    settings_path: Option<PathBuf>,
    /// The Repository the window last showed. It stays on this machine and is read with the settings, so a
    /// normal launch knows what to reopen before the window exists.
    last_repository: Option<RepositoryName>,
}

/// The two files and what was last read from them.
pub struct SettingsFiles {
    folders: Folders,
    /// The last good `machine.toml`. Until there is one, Tsugiki can't know which settings file is meant.
    machine: Option<Machine>,
    loaded: LoadedSettings,
}

impl SettingsFiles {
    /// Reads both files. Anything wrong with them leaves the defaults in force.
    pub fn load(folders: Folders) -> Self {
        let mut files = Self {
            folders,
            machine: None,
            loaded: LoadedSettings::default(),
        };
        files.reload();
        files
    }

    pub fn loaded(&self) -> &LoadedSettings {
        &self.loaded
    }

    pub fn view(&self) -> SettingsView {
        SettingsView {
            loaded: self.loaded.clone(),
            home: self.home(),
        }
    }

    /// Reads both files again, for the Settings window to call as it opens.
    pub fn reload(&mut self) {
        let mut problems = Vec::new();
        match read_machine(&self.folders.machine_file()) {
            Ok(machine) => self.machine = Some(machine),
            Err(problem) => problems.push(problem),
        }
        if let Some(machine) = &self.machine {
            match read_settings(&self.folders, machine, &self.loaded.settings) {
                Ok(settings) => self.loaded.settings = settings,
                Err(problem) => problems.push(problem),
            }
        }
        for problem in &problems {
            log::warn!("{problem}");
        }
        self.loaded.problems = problems;
    }

    /// Saves `settings`, keeping everything else in the file, and reads the files again. The Settings
    /// window's Apply calls this.
    ///
    /// It works from what the files say now, so an edit made since the last read stays. It refuses while
    /// either file can't be used: a broken `machine.toml` might point somewhere else, and a broken
    /// `settings.toml` might hold work the user hasn't finished.
    pub fn write(&mut self, settings: &Settings) -> Result<(), Error> {
        let machine = self.machine_to_edit()?;
        let path = settings_file(&self.folders, &machine);
        let mut edit = TomlEdit::<Settings>::open(&path)?;
        merge(
            edit.document.as_table_mut(),
            without_repositories(as_document(settings)).as_table(),
            without_repositories(as_document(&Settings::default())).as_table(),
        );
        edit.save()?;
        self.reload();
        Ok(())
    }

    /// What the window opens on: the last Repository if the settings still hold it, or else the first.
    pub fn home(&self) -> Home {
        let repositories = &self.loaded.settings.repositories;
        let last = self
            .machine
            .as_ref()
            .and_then(|machine| machine.last_repository.as_ref())
            .and_then(|last| repositories.iter().find(|one| one.name.is_same(last)));
        match last.or_else(|| repositories.first()) {
            Some(repository) => Home::Repository {
                repository: repository.clone(),
            },
            None => Home::default(),
        }
    }

    /// Adds `name` to the settings, unless they hold it already, and makes it what the window opens on.
    ///
    /// Like `write`, it works from what the files say now and refuses while either can't be used. It saves
    /// `machine.toml` first: if that fails, nothing has changed. If `settings.toml` then fails, the name
    /// there holds nothing, and `home` passes over it.
    pub fn add_repository(&mut self, name: &RepositoryName) -> Result<Home, Error> {
        let machine = self.machine_to_edit()?;
        let path = settings_file(&self.folders, &machine);
        let mut edit = TomlEdit::<Settings>::open(&path)?;
        let held = edit
            .values
            .repositories
            .iter()
            .any(|one| one.name.is_same(name));
        if !held {
            edit.append(&RepositorySettings::new(name.clone()));
        }
        remember_last_repository(&self.folders.machine_file(), name)?;
        edit.save()?;
        self.reload();
        Ok(self.home())
    }

    fn machine_to_edit(&self) -> Result<Machine, Error> {
        read_machine(&self.folders.machine_file()).map_err(|problem| refused(&problem))
    }
}

/// The thread that owns the settings files, so saving never waits on GitHub and GitHub never waits on a slow
/// drive. Without folders it has nothing to save to, and each add answers with that problem.
///
/// Ending the process while it writes can leave a file cut short, as it could on any thread.
#[derive(Clone)]
pub struct SettingsOwner {
    additions: Sender<Addition>,
    view: Arc<Latest<SettingsView>>,
}

struct Addition {
    name: RepositoryName,
    reply: Sender<Result<SettingsView, Error>>,
}

impl SettingsOwner {
    /// From here on only this thread can read `files`, and `view` answers from its copy.
    pub fn start(mut files: Result<SettingsFiles, Error>) -> Self {
        let first = files.as_ref().map(SettingsFiles::view).unwrap_or_default();
        let view = Arc::new(Latest::new(first));
        let copy = Arc::clone(&view);
        let (additions, incoming) = mpsc::channel::<Addition>();
        let _ = thread::spawn(move || {
            for Addition { name, reply } in incoming {
                let saved = match &mut files {
                    Ok(files) => files.add_repository(&name).map(|_| files.view()),
                    Err(problem) => Err(problem.clone()),
                };
                // Before the answer, so whoever asked finds the addition in `view`. Additions come through
                // here one at a time, so an older one can't replace a newer one.
                if let Ok(saved) = &saved {
                    copy.set(saved);
                }
                #[expect(
                    clippy::let_underscore_must_use,
                    reason = "this fails only if the caller has gone, and it waits for this answer"
                )]
                let _ = reply.send(saved);
            }
        });
        Self { additions, view }
    }

    /// What the settings said after the last addition saved. This never waits on a file.
    pub fn view(&self) -> SettingsView {
        self.view.get()
    }

    /// Adds `name` to the settings, and answers with what they say now.
    pub fn add_repository(&self, name: RepositoryName) -> Result<SettingsView, Error> {
        let (reply, answer) = mpsc::channel();
        self.additions
            .send(Addition { name, reply })
            .ok()
            .and_then(|()| answer.recv().ok())
            .unwrap_or_else(|| Err(Error::new(ErrorKind::Internal, STOPPED_SAVING)))
    }
}

/// A settings file as it says now, open for changes that keep the user's comments and order.
struct TomlEdit<'a, T> {
    path: &'a Path,
    before: String,
    values: T,
    document: DocumentMut,
}

impl<'a, T: DeserializeOwned> TomlEdit<'a, T> {
    fn open(path: &'a Path) -> Result<Self, Error> {
        let before = read_text(path)
            .map_err(|problem| refused(&problem))?
            .unwrap_or_default();
        let parsed = toml_edit::de::from_str::<T>(&before).and_then(|values| {
            let document = before.parse::<DocumentMut>()?;
            Ok((values, document))
        });
        let (values, document) =
            parsed.map_err(|cause| refused(&located(path, &before, &cause)))?;
        Ok(Self {
            path,
            before,
            values,
            document,
        })
    }

    fn save(self) -> Result<(), Error> {
        let merged = in_the_files_style(self.document.to_string(), &self.before);
        if merged == self.before {
            return Ok(());
        }
        save(self.path, &merged).map_err(|cause| {
            refused(&format!(
                "Tsugiki can't save {}: {cause}",
                self.path.display()
            ))
        })
    }
}

impl TomlEdit<'_, Settings> {
    /// Adds a Repository after the others, in the form the file already uses for them.
    fn append(&mut self, repository: &RepositorySettings) {
        let table = as_document(repository).as_table().clone();
        let repositories = self
            .document
            .entry(REPOSITORIES)
            .or_insert_with(|| Item::ArrayOfTables(ArrayOfTables::new()));
        if let Some(tables) = repositories.as_array_of_tables_mut() {
            tables.push(table);
        } else if let Some(written_inline) = repositories.as_array_mut() {
            written_inline.push(table.into_inline_table());
        }
    }
}

/// `write` saves what the Settings window changes, and that window doesn't edit the Repository list. Merging
/// the list would replace the file's tables, and the user's comments on them.
fn without_repositories(mut document: DocumentMut) -> DocumentMut {
    drop(document.remove(REPOSITORIES));
    document
}

fn remember_last_repository(path: &Path, name: &RepositoryName) -> Result<(), Error> {
    let mut edit = TomlEdit::<Machine>::open(path)?;
    let wanted = toml_edit::value(name.to_string());
    merge_value(edit.document.as_table_mut(), LAST_REPOSITORY, &wanted, None);
    edit.save()
}

fn refused(problem: &str) -> Error {
    log::warn!("Tsugiki didn't save the settings: {problem}");
    Error::new(ErrorKind::Internal, problem)
}

#[expect(
    clippy::expect_used,
    reason = "these structs hold only tables and plain values, which TOML can always write"
)]
fn as_document<T: Serialize>(value: &T) -> DocumentMut {
    toml_edit::ser::to_document(value).expect("settings are always TOML")
}

/// Makes `file` say what `wanted` says, writing only what differs from `defaults`.
fn merge(file: &mut dyn TableLike, wanted: &dyn TableLike, defaults: &dyn TableLike) {
    for (key, item) in wanted.iter() {
        let default = defaults.get(key);
        match (item.as_table_like(), default.and_then(Item::as_table_like)) {
            (Some(wanted), Some(defaults)) => merge_section(file, key, wanted, defaults),
            _ => merge_value(file, key, item, default),
        }
    }
}

fn merge_section(
    file: &mut dyn TableLike,
    key: &str,
    wanted: &dyn TableLike,
    defaults: &dyn TableLike,
) {
    if file.get(key).is_none() {
        drop(file.insert(key, Item::Table(Table::new())));
    }
    if let Some(section) = file.get_mut(key).and_then(Item::as_table_like_mut) {
        merge(section, wanted, defaults);
    }
    // A section with nothing left in it goes, unless the user wrote a comment on it.
    let empty = file
        .get(key)
        .and_then(Item::as_table_like)
        .is_some_and(TableLike::is_empty);
    if empty && !has_comment(file, key) {
        drop(file.remove(key));
    }
}

/// A value back at its default goes, unless the user wrote a comment on it. Their words matter more than a
/// tidy file, so that value stays, at its default.
fn merge_value(file: &mut dyn TableLike, key: &str, wanted: &Item, default: Option<&Item>) {
    if default.is_some_and(|default| same(wanted, default)) && !has_comment(file, key) {
        drop(file.remove(key));
        return;
    }
    match (
        file.get_mut(key).and_then(Item::as_value_mut),
        wanted.as_value(),
    ) {
        (Some(current), Some(new)) => {
            if current.to_string().trim() != new.to_string().trim() {
                let decor = current.decor().clone();
                *current = new.clone();
                *current.decor_mut() = decor;
            }
        }
        _ => drop(file.insert(key, wanted.clone())),
    }
}

fn same(one: &Item, other: &Item) -> bool {
    one.to_string().trim() == other.to_string().trim()
}

/// Whether the user wrote a comment above or beside this key, or beside a section's header.
fn has_comment(file: &dyn TableLike, key: &str) -> bool {
    let item = file.get(key);
    [
        file.key(key).map(Key::leaf_decor),
        item.and_then(Item::as_value).map(Value::decor),
        item.and_then(Item::as_table).map(Table::decor),
    ]
    .into_iter()
    .flatten()
    .flat_map(|decor| [decor.prefix(), decor.suffix()])
    .flatten()
    .filter_map(RawString::as_str)
    .any(|text| text.contains('#'))
}

/// `toml_edit` writes `\n` and no byte order mark, and a file shouldn't change on every line because of that.
fn in_the_files_style(merged: String, before: &str) -> String {
    let merged = if before.contains("\r\n") {
        merged.replace('\n', "\r\n")
    } else {
        merged
    };
    if before.starts_with('\u{feff}') {
        format!("\u{feff}{merged}")
    } else {
        merged
    }
}

/// Writes over the file rather than moving a new one onto it, so a symlink and the file's permissions stay,
/// and a failure can't leave a second file in a dotfiles repository.
fn save(path: &Path, text: &str) -> io::Result<()> {
    fs::create_dir_all(path.with_file_name(""))?;
    fs::write(path, text)
}

fn read_machine(path: &Path) -> Result<Machine, String> {
    let machine = read::<Machine>(path)?.unwrap_or_default();
    let starts_with_tilde = machine
        .settings_path
        .as_ref()
        .is_some_and(|moved| moved.starts_with("~"));
    if starts_with_tilde {
        return Err(format!(
            "{}: settings_path can't start with ~, because Tsugiki doesn't expand it. Write the full path.",
            path.display()
        ));
    }
    Ok(machine)
}

/// The settings file's values. With no file in the usual place, the defaults are what the user asked for.
/// With none where `settings_path` points, something is wrong, and it may only be a drive gone for now.
fn read_settings(
    folders: &Folders,
    machine: &Machine,
    current: &Settings,
) -> Result<Settings, String> {
    let path = settings_file(folders, machine);
    match read::<Settings>(&path)? {
        Some(settings) => Ok(settings),
        None if machine.settings_path.is_none() => Ok(Settings::default()),
        None => {
            let kept = if *current == Settings::default() {
                "so Tsugiki is using the defaults"
            } else {
                "so Tsugiki keeps the last values it read"
            };
            Err(format!(
                "There is no settings file at {}, {kept}.",
                path.display()
            ))
        }
    }
}

fn settings_file(folders: &Folders, machine: &Machine) -> PathBuf {
    machine
        .settings_path
        .as_ref()
        .map_or_else(|| folders.settings_file(), |moved| folders.data.join(moved))
}

/// The file's text, or `None` if it isn't there. A problem comes back in words for the status bar.
fn read_text(path: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(cause) if cause.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(cause) => Err(format!("Tsugiki can't read {}: {cause}", path.display())),
    }
}

/// The file's values, or `None` if it isn't there.
fn read<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    read_text(path)?
        .map(|text| toml_edit::de::from_str(&text).map_err(|cause| located(path, &text, &cause)))
        .transpose()
}

/// Names the file and, when the parser knows it, the line.
fn located(path: &Path, text: &str, cause: &toml_edit::de::Error) -> String {
    let line = cause.span().map_or_else(String::new, |span| {
        let earlier_lines = text
            .bytes()
            .take(span.start)
            .filter(|b| *b == b'\n')
            .count();
        format!(", line {}", earlier_lines.saturating_add(1))
    });
    format!("{}{line}: {}", path.display(), cause.message())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conventions::Conventions;

    const HIDDEN: Settings = Settings {
        status_bar: StatusBarSettings { visible: false },
        repositories: Vec::new(),
    };

    fn repository(text: &str) -> RepositoryName {
        RepositoryName::parse(text).unwrap()
    }

    fn opened(name: &str, conventions: Conventions) -> Home {
        Home::Repository {
            repository: RepositorySettings {
                name: repository(name),
                conventions,
            },
        }
    }

    /// Separate data and settings folders, as an installed Tsugiki on Windows has, and the folder around
    /// them.
    fn temp_folders(name: &str) -> (Folders, PathBuf) {
        let root = crate::testing::temp_path(&format!("settings-{name}"));
        let folders = Folders {
            data: root.join("data"),
            settings: root.join("settings"),
        };
        fs::create_dir_all(&folders.data).unwrap();
        fs::create_dir_all(&folders.settings).unwrap();
        (folders, root)
    }

    /// What `settings.toml` says after Tsugiki saves `settings` over `before`.
    fn saved_over(name: &str, before: &str, settings: &Settings) -> String {
        let (folders, root) = temp_folders(name);
        fs::write(folders.settings_file(), before).unwrap();

        SettingsFiles::load(folders.clone())
            .write(settings)
            .unwrap();
        let after = fs::read_to_string(folders.settings_file()).unwrap();
        fs::remove_dir_all(root).unwrap();
        after
    }

    /// Writes `settings_path` into `machine.toml`.
    fn point_at(folders: &Folders, path: &Path) {
        let path = path.to_str().unwrap().replace('\\', "/");
        fs::write(
            folders.machine_file(),
            format!("settings_path = \"{path}\"\n"),
        )
        .unwrap();
    }

    fn only_problem(files: &SettingsFiles) -> String {
        let problems = &files.loaded().problems;
        assert_eq!(problems.len(), 1, "{problems:?}");
        problems.first().unwrap().clone()
    }

    #[test]
    fn with_no_files_tsugiki_starts_from_the_defaults_and_creates_nothing() {
        let (folders, root) = temp_folders("no-files");

        let files = SettingsFiles::load(folders.clone());
        let created = folders.settings_file().exists() || folders.machine_file().exists();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(files.loaded(), &LoadedSettings::default());
        assert!(files.loaded().settings.status_bar.visible);
        assert!(!created);
    }

    #[test]
    fn a_value_in_settings_toml_is_read_at_start() {
        let (folders, root) = temp_folders("read");
        fs::write(folders.settings_file(), "[status_bar]\nvisible = false\n").unwrap();

        let files = SettingsFiles::load(folders);
        fs::remove_dir_all(root).unwrap();

        assert_eq!(files.loaded().settings, HIDDEN);
        assert!(files.loaded().problems.is_empty());
    }

    #[test]
    fn values_a_file_leaves_out_or_this_version_doesnt_know_change_nothing() {
        let (folders, root) = temp_folders("unknown");
        let text = "from_a_newer_tsugiki = 1\n[status_bar]\nanother = 2\n";
        fs::write(folders.settings_file(), text).unwrap();

        let files = SettingsFiles::load(folders);
        fs::remove_dir_all(root).unwrap();

        assert_eq!(files.loaded(), &LoadedSettings::default());
    }

    #[test]
    fn a_file_that_doesnt_parse_is_left_alone_and_named_with_its_line() {
        let (folders, root) = temp_folders("broken");
        let text = "# Mine.\n[status_bar]\nvisible = = false\n";
        fs::write(folders.settings_file(), text).unwrap();

        let files = SettingsFiles::load(folders.clone());
        let after = fs::read_to_string(folders.settings_file()).unwrap();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(after, text);
        assert_eq!(files.loaded().settings, Settings::default());
        let problem = only_problem(&files);
        assert!(problem.contains("settings.toml, line 3"), "{problem}");
    }

    #[test]
    fn a_value_of_the_wrong_type_is_named_with_its_line() {
        let (folders, root) = temp_folders("wrong-type");
        fs::write(
            folders.settings_file(),
            "\n[status_bar]\nvisible = \"no\"\n",
        )
        .unwrap();

        let files = SettingsFiles::load(folders);
        fs::remove_dir_all(root).unwrap();

        let problem = only_problem(&files);
        assert!(problem.contains("settings.toml, line 3"), "{problem}");
    }

    #[test]
    fn a_file_it_cant_read_is_a_problem_that_names_it() {
        let (folders, root) = temp_folders("unreadable");
        fs::create_dir(folders.settings_file()).unwrap();

        let files = SettingsFiles::load(folders);
        fs::remove_dir_all(root).unwrap();

        assert_eq!(files.loaded().settings, Settings::default());
        let problem = only_problem(&files);
        assert!(problem.contains("settings.toml"), "{problem}");
    }

    #[test]
    fn a_reload_picks_up_an_edit() {
        let (folders, root) = temp_folders("reload");
        let mut files = SettingsFiles::load(folders.clone());
        assert_eq!(files.loaded().settings, Settings::default());

        fs::write(folders.settings_file(), "[status_bar]\nvisible = false\n").unwrap();
        files.reload();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(files.loaded().settings, HIDDEN);
    }

    #[test]
    fn a_reload_of_a_broken_file_keeps_the_last_good_values_until_it_is_fixed() {
        let (folders, root) = temp_folders("last-good");
        let file = folders.settings_file();
        fs::write(&file, "[status_bar]\nvisible = false\n").unwrap();
        let mut files = SettingsFiles::load(folders);

        fs::write(&file, "[status_bar\n").unwrap();
        files.reload();
        let while_broken = files.loaded().clone();

        fs::write(&file, "[status_bar]\nvisible = true\n").unwrap();
        files.reload();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(while_broken.settings, HIDDEN);
        assert_eq!(while_broken.problems.len(), 1);
        assert_eq!(files.loaded(), &LoadedSettings::default());
    }

    #[test]
    fn a_settings_toml_removed_from_its_usual_folder_means_the_defaults_again() {
        let (folders, root) = temp_folders("removed");
        fs::write(folders.settings_file(), "[status_bar]\nvisible = false\n").unwrap();
        let mut files = SettingsFiles::load(folders.clone());

        fs::remove_file(folders.settings_file()).unwrap();
        files.reload();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(files.loaded(), &LoadedSettings::default());
    }

    #[test]
    fn settings_path_moves_settings_toml() {
        let (folders, root) = temp_folders("moved");
        let moved = root.join("dotfiles.toml");
        fs::write(&moved, "[status_bar]\nvisible = false\n").unwrap();
        fs::write(folders.settings_file(), "[status_bar]\nvisible = true\n").unwrap();
        point_at(&folders, &moved);

        let files = SettingsFiles::load(folders);
        fs::remove_dir_all(root).unwrap();

        assert_eq!(files.loaded().settings, HIDDEN);
        assert!(files.loaded().problems.is_empty());
    }

    #[test]
    fn a_relative_settings_path_starts_at_machine_toml() {
        let (folders, root) = temp_folders("relative");
        fs::write(
            folders.data.join("beside.toml"),
            "[status_bar]\nvisible = false\n",
        )
        .unwrap();
        fs::write(folders.machine_file(), "settings_path = \"beside.toml\"\n").unwrap();

        let files = SettingsFiles::load(folders);
        fs::remove_dir_all(root).unwrap();

        assert_eq!(files.loaded().settings, HIDDEN);
    }

    #[test]
    fn a_missing_file_at_settings_path_starts_from_the_defaults_and_says_so() {
        let (folders, root) = temp_folders("moved-missing");
        fs::write(folders.settings_file(), "[status_bar]\nvisible = false\n").unwrap();
        point_at(&folders, &root.join("not-there.toml"));

        let files = SettingsFiles::load(folders);
        fs::remove_dir_all(root).unwrap();

        assert_eq!(files.loaded().settings, Settings::default());
        let problem = only_problem(&files);
        assert!(problem.contains("not-there.toml"), "{problem}");
        assert!(problem.contains("defaults"), "{problem}");
    }

    #[test]
    fn a_broken_machine_toml_is_named_with_its_line_and_keeps_the_last_good_path() {
        let (folders, root) = temp_folders("machine-broken");
        let moved = root.join("dotfiles.toml");
        fs::write(&moved, "[status_bar]\nvisible = false\n").unwrap();
        point_at(&folders, &moved);
        let mut files = SettingsFiles::load(folders.clone());

        fs::write(folders.machine_file(), "\nsettings_path =\n").unwrap();
        files.reload();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(files.loaded().settings, HIDDEN);
        let problem = only_problem(&files);
        assert!(problem.contains("machine.toml, line 2"), "{problem}");
    }

    #[test]
    fn saving_writes_only_what_differs_from_the_defaults() {
        let (folders, root) = temp_folders("save-new");
        let mut files = SettingsFiles::load(folders.clone());

        files.write(&HIDDEN).unwrap();
        let after = fs::read_to_string(folders.settings_file()).unwrap();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(after, "[status_bar]\nvisible = false\n");
        assert_eq!(files.loaded().settings, HIDDEN);
    }

    #[test]
    fn saving_the_defaults_over_no_file_creates_no_file() {
        let (folders, root) = temp_folders("save-defaults");

        SettingsFiles::load(folders.clone())
            .write(&Settings::default())
            .unwrap();
        let created = folders.settings_file().exists();
        fs::remove_dir_all(root).unwrap();

        assert!(!created);
    }

    #[test]
    fn saving_creates_the_folder_it_needs() {
        let (folders, root) = temp_folders("save-folder");
        fs::remove_dir(&folders.settings).unwrap();

        SettingsFiles::load(folders.clone()).write(&HIDDEN).unwrap();
        let created = folders.settings_file().exists();
        fs::remove_dir_all(root).unwrap();

        assert!(created);
    }

    #[test]
    fn saving_keeps_comments_order_and_values_it_doesnt_know() {
        let before = "\
# My settings.

zebra = 1 # From a newer Tsugiki.

# The bar.
[status_bar]
another = 2

# Later.
[later]
alpha = \"x\"
";
        let after = saved_over("save-keeps", before, &HIDDEN);

        assert_eq!(
            after,
            "\
# My settings.

zebra = 1 # From a newer Tsugiki.

# The bar.
[status_bar]
another = 2
visible = false

# Later.
[later]
alpha = \"x\"
"
        );
    }

    #[test]
    fn changing_a_value_keeps_the_comment_beside_it() {
        let before = "[status_bar]\nvisible = true # For now.\n";
        let after = saved_over("save-beside", before, &HIDDEN);
        assert_eq!(after, "[status_bar]\nvisible = false # For now.\n");
    }

    #[test]
    fn a_value_back_at_its_default_goes_with_a_section_left_empty() {
        let before = "first = 1\n\n[status_bar]\nvisible = false\n";
        let after = saved_over("save-removes", before, &Settings::default());
        assert_eq!(after, "first = 1\n");
    }

    #[test]
    fn a_value_back_at_its_default_stays_if_the_user_commented_on_it() {
        let above = "[status_bar]\n# Hidden for screenshots.\nvisible = false\n";
        assert_eq!(
            saved_over("save-above", above, &Settings::default()),
            "[status_bar]\n# Hidden for screenshots.\nvisible = true\n"
        );
        let beside = "[status_bar]\nvisible = false # Screenshots.\n";
        assert_eq!(
            saved_over("save-beside-default", beside, &Settings::default()),
            "[status_bar]\nvisible = true # Screenshots.\n"
        );
    }

    #[test]
    fn a_section_with_a_comment_above_it_stays_when_it_empties() {
        let before = "# The bar.\n[status_bar]\nvisible = false\n";
        let after = saved_over("save-section", before, &Settings::default());
        assert_eq!(after, "# The bar.\n[status_bar]\n");
    }

    #[test]
    fn a_section_written_inline_or_dotted_is_changed_where_it_is() {
        assert_eq!(
            saved_over("save-inline", "status_bar = { visible = true }\n", &HIDDEN),
            "status_bar = { visible = false }\n"
        );
        assert_eq!(
            saved_over("save-dotted", "status_bar.visible = true\n", &HIDDEN),
            "status_bar.visible = false\n"
        );
    }

    #[test]
    fn saving_keeps_an_edit_made_since_the_last_read() {
        let (folders, root) = temp_folders("save-edit");
        let mut files = SettingsFiles::load(folders.clone());

        fs::write(folders.settings_file(), "# Added by hand.\nmine = 1\n").unwrap();
        files.write(&HIDDEN).unwrap();
        let after = fs::read_to_string(folders.settings_file()).unwrap();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(
            after,
            "# Added by hand.\nmine = 1\n\n[status_bar]\nvisible = false\n"
        );
    }

    #[test]
    fn saving_follows_settings_path() {
        let (folders, root) = temp_folders("save-moved");
        let moved = root.join("dotfiles.toml");
        point_at(&folders, &moved);
        let mut files = SettingsFiles::load(folders.clone());

        files.write(&HIDDEN).unwrap();
        let after = fs::read_to_string(&moved).unwrap();
        let usual_folder_used = folders.settings_file().exists();
        let loaded = files.loaded().clone();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(after, "[status_bar]\nvisible = false\n");
        assert!(!usual_folder_used);
        assert_eq!(loaded.settings, HIDDEN);
        assert!(loaded.problems.is_empty(), "the file is there now");
    }

    #[test]
    fn saving_over_a_file_that_doesnt_parse_refuses_and_leaves_it_alone() {
        for (name, text) in [
            ("save-broken", "\n[status_bar\n"),
            ("save-wrong-type", "\n[status_bar]\nvisible = \"no\"\n"),
        ] {
            let (folders, root) = temp_folders(name);
            fs::write(folders.settings_file(), text).unwrap();
            let mut files = SettingsFiles::load(folders.clone());

            let refused = files.write(&HIDDEN).unwrap_err();
            let after = fs::read_to_string(folders.settings_file()).unwrap();
            fs::remove_dir_all(root).unwrap();

            assert_eq!(after, text);
            assert!(
                refused.message.contains("settings.toml, line"),
                "{}",
                refused.message
            );
            assert_eq!(files.loaded().settings, Settings::default());
        }
    }

    #[test]
    fn a_broken_machine_toml_refuses_rather_than_saving_to_the_wrong_place() {
        let (folders, root) = temp_folders("save-machine");
        fs::write(folders.machine_file(), "settings_path =\n").unwrap();
        let mut files = SettingsFiles::load(folders.clone());

        let refused = files.write(&HIDDEN).unwrap_err();
        let created = folders.settings_file().exists();
        fs::remove_dir_all(root).unwrap();

        assert!(!created);
        assert_eq!(refused.kind, ErrorKind::Internal);
        assert!(
            refused.message.contains("machine.toml, line 1"),
            "{}",
            refused.message
        );
    }

    #[test]
    fn a_settings_file_it_cant_read_refuses() {
        let (folders, root) = temp_folders("save-unreadable");
        fs::create_dir(folders.settings_file()).unwrap();
        let mut files = SettingsFiles::load(folders.clone());

        let refused = files.write(&HIDDEN).unwrap_err();
        fs::remove_dir_all(root).unwrap();

        assert!(
            refused.message.contains("settings.toml"),
            "{}",
            refused.message
        );
    }

    #[test]
    #[expect(
        clippy::permissions_set_readonly_false,
        reason = "the test's own file, made writable again only so it can be removed"
    )]
    fn a_save_that_fails_says_so_and_leaves_the_file_alone() {
        let (folders, root) = temp_folders("save-fails");
        let text = "mine = 1\n";
        let file = folders.settings_file();
        fs::write(&file, text).unwrap();
        let mut permissions = fs::metadata(&file).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&file, permissions.clone()).unwrap();
        let mut files = SettingsFiles::load(folders);

        let refused = files.write(&HIDDEN).unwrap_err();
        let after = fs::read_to_string(&file).unwrap();
        permissions.set_readonly(false);
        fs::set_permissions(&file, permissions).unwrap();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(after, text);
        assert!(
            refused.message.contains("settings.toml"),
            "{}",
            refused.message
        );
        assert_eq!(files.loaded().settings, Settings::default());
    }

    #[test]
    fn a_comment_on_a_section_keeps_it_when_it_empties() {
        let cases = [
            (
                "save-header",
                "[status_bar] # Note.\nvisible = false\n",
                "[status_bar] # Note.\n",
            ),
            (
                "save-inline-beside",
                "status_bar = { visible = false } # Note.\n",
                "status_bar = {} # Note.\n",
            ),
            (
                "save-inline-above",
                "# Note.\nstatus_bar = { visible = false }\n",
                "# Note.\nstatus_bar = {}\n",
            ),
        ];
        for (name, before, expected) in cases {
            assert_eq!(saved_over(name, before, &Settings::default()), expected);
        }
    }

    #[test]
    fn saving_keeps_a_files_line_endings_and_byte_order_mark() {
        assert_eq!(
            saved_over("save-crlf", "# Mine.\r\nmine = 1\r\n", &HIDDEN),
            "# Mine.\r\nmine = 1\r\n\r\n[status_bar]\r\nvisible = false\r\n"
        );
        assert_eq!(
            saved_over("save-bom", "\u{feff}mine = 1\n", &HIDDEN),
            "\u{feff}mine = 1\n\n[status_bar]\nvisible = false\n"
        );
    }

    #[test]
    fn saving_what_a_file_already_says_changes_nothing() {
        let before = "\u{feff}# Mine.\r\n[status_bar]\r\nvisible = false\r\n";
        assert_eq!(saved_over("save-same", before, &HIDDEN), before);
    }

    #[test]
    fn a_settings_path_starting_with_a_tilde_is_a_problem_not_a_folder() {
        let (folders, root) = temp_folders("tilde");
        fs::write(folders.settings_file(), "[status_bar]\nvisible = false\n").unwrap();
        let machine = "settings_path = \"~/dotfiles/settings.toml\"\n";
        fs::write(folders.machine_file(), machine).unwrap();

        let mut files = SettingsFiles::load(folders.clone());
        let refused = files.write(&HIDDEN).unwrap_err();
        let tilde_folder = folders.data.join("~").exists();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(files.loaded().settings, Settings::default());
        let problem = only_problem(&files);
        assert!(problem.contains("machine.toml"), "{problem}");
        assert!(problem.contains('~'), "{problem}");
        assert!(refused.message.contains('~'), "{}", refused.message);
        assert!(!tilde_folder);
    }

    #[test]
    fn a_machine_toml_broken_from_the_start_doesnt_read_a_settings_file_it_might_not_mean() {
        let (folders, root) = temp_folders("machine-broken-start");
        fs::write(folders.settings_file(), "[status_bar]\nvisible = false\n").unwrap();
        fs::write(folders.machine_file(), "settings_path =\n").unwrap();

        let files = SettingsFiles::load(folders);
        fs::remove_dir_all(root).unwrap();

        assert_eq!(files.loaded().settings, Settings::default());
        assert_eq!(files.loaded().problems.len(), 1);
    }

    #[test]
    fn the_owner_saves_an_addition_and_answers_with_what_the_files_say() {
        let (folders, root) = temp_folders("owner");
        let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders.clone())));

        let saved = owner.add_repository(repository("python/cpython")).unwrap();
        let text = fs::read_to_string(folders.settings_file()).unwrap();
        fs::remove_dir_all(root).unwrap();

        assert!(text.contains("python/cpython"), "{text}");
        assert_eq!(saved.home, opened("python/cpython", Conventions::Cpython));
        assert_eq!(saved.loaded.settings.repositories.len(), 1);
    }

    #[test]
    fn the_owner_passes_on_why_a_file_refused_an_addition() {
        let (folders, root) = temp_folders("owner-refused");
        fs::write(folders.settings_file(), "[status_bar\n").unwrap();
        let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders)));

        let refused = owner.add_repository(repository("a/b")).unwrap_err();
        fs::remove_dir_all(root).unwrap();

        assert!(
            refused.message.contains("settings.toml"),
            "{}",
            refused.message
        );
    }

    #[test]
    fn without_folders_the_owner_says_what_went_wrong_with_them() {
        let lost = Error::new(ErrorKind::Internal, "HOME isn't a full path.");
        let owner = SettingsOwner::start(Err(lost));

        let refused = owner.add_repository(repository("a/b")).unwrap_err();

        assert_eq!(refused.message, "HOME isn't a full path.");
    }

    #[test]
    fn the_owner_answers_with_what_the_files_said_when_it_started() {
        let (folders, root) = temp_folders("owner-started");
        fs::write(folders.settings_file(), "[status_bar\n").unwrap();

        let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders)));
        fs::remove_dir_all(root).unwrap();

        assert_eq!(owner.view().loaded.problems.len(), 1);
        assert_eq!(owner.view().home, Home::default());
    }

    #[test]
    fn without_folders_the_owner_answers_with_the_defaults() {
        let lost = Error::new(ErrorKind::Internal, "HOME isn't a full path.");

        let owner = SettingsOwner::start(Err(lost));

        assert_eq!(owner.view(), SettingsView::default());
    }

    #[test]
    fn an_addition_is_in_the_view_by_the_time_the_owner_answers() {
        let (folders, root) = temp_folders("owner-view");
        let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders)));

        let saved = owner.add_repository(repository("python/cpython")).unwrap();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(owner.view(), saved);
    }

    #[test]
    fn an_addition_that_fails_leaves_the_view_alone() {
        let (folders, root) = temp_folders("owner-view-refused");
        fs::write(folders.settings_file(), "[status_bar\n").unwrap();
        let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders)));
        let before = owner.view();

        let refused = owner.add_repository(repository("a/b"));
        fs::remove_dir_all(root).unwrap();

        assert!(refused.is_err());
        assert_eq!(owner.view(), before);
    }

    #[test]
    fn additions_that_overlap_all_reach_the_view() {
        let (folders, root) = temp_folders("owner-overlap");
        let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders)));

        thread::scope(|scope| {
            for name in ["a/one", "a/two", "a/three", "a/four"] {
                let owner = owner.clone();
                let _ = scope.spawn(move || owner.add_repository(repository(name)).unwrap());
            }
        });
        fs::remove_dir_all(root).unwrap();

        assert_eq!(owner.view().loaded.settings.repositories.len(), 4);
    }

    #[test]
    fn an_owner_whose_thread_has_gone_says_so() {
        let owner = SettingsOwner {
            additions: mpsc::channel().0,
            view: Arc::default(),
        };

        let refused = owner.add_repository(repository("a/b")).unwrap_err();

        assert_eq!(refused.message, STOPPED_SAVING);
    }

    #[test]
    fn with_no_repositories_the_window_opens_on_onboarding() {
        let (folders, root) = temp_folders("home-none");
        let files = SettingsFiles::load(folders);
        fs::remove_dir_all(root).unwrap();
        assert_eq!(files.home(), Home::default());
    }

    #[test]
    fn adding_a_repository_saves_it_with_its_conventions() {
        let (folders, root) = temp_folders("add");
        let mut files = SettingsFiles::load(folders.clone());

        let home = files.add_repository(&repository("python/cpython")).unwrap();
        let text = fs::read_to_string(folders.settings_file()).unwrap();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(home, opened("python/cpython", Conventions::Cpython));
        assert_eq!(
            text,
            "[[repositories]]\nname = \"python/cpython\"\nconventions = \"cpython\"\n"
        );
        assert_eq!(files.loaded().settings.repositories.len(), 1);
    }

    #[test]
    fn another_repository_goes_after_the_others_and_keeps_the_users_words() {
        let (folders, root) = temp_folders("add-second");
        let before = "\
# Mine.
[[repositories]]
name = \"python/cpython\" # The big one.
conventions = \"cpython\"

# The bar.
[status_bar]
visible = false
";
        fs::write(folders.settings_file(), before).unwrap();
        let mut files = SettingsFiles::load(folders.clone());

        let home = files
            .add_repository(&repository("sergey-miryanov/gcmon"))
            .unwrap();
        let after = fs::read_to_string(folders.settings_file()).unwrap();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(home, opened("sergey-miryanov/gcmon", Conventions::Github));
        assert_eq!(
            after,
            "\
# Mine.
[[repositories]]
name = \"python/cpython\" # The big one.
conventions = \"cpython\"

[[repositories]]
name = \"sergey-miryanov/gcmon\"
conventions = \"github\"

# The bar.
[status_bar]
visible = false
"
        );
    }

    #[test]
    fn a_list_written_inline_stays_inline() {
        let (folders, root) = temp_folders("add-inline");
        fs::write(
            folders.settings_file(),
            "repositories = [{ name = \"a/b\" }]\n",
        )
        .unwrap();
        let mut files = SettingsFiles::load(folders.clone());

        drop(files.add_repository(&repository("c/d")).unwrap());
        let names: Vec<_> = files
            .loaded()
            .settings
            .repositories
            .iter()
            .map(|one| one.name.to_string())
            .collect();
        let after = fs::read_to_string(folders.settings_file()).unwrap();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(names, ["a/b", "c/d"]);
        assert!(after.starts_with("repositories = [{"), "{after}");
    }

    #[test]
    fn adding_a_repository_twice_keeps_one_and_opens_it() {
        let (folders, root) = temp_folders("add-twice");
        let mut files = SettingsFiles::load(folders.clone());
        drop(files.add_repository(&repository("python/cpython")).unwrap());
        drop(files.add_repository(&repository("a/b")).unwrap());
        let before = fs::read_to_string(folders.settings_file()).unwrap();

        let home = files.add_repository(&repository("Python/CPython")).unwrap();
        let after = fs::read_to_string(folders.settings_file()).unwrap();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(after, before);
        assert_eq!(home, opened("python/cpython", Conventions::Cpython));
    }

    #[test]
    fn a_normal_launch_reopens_the_last_repository() {
        let (folders, root) = temp_folders("reopen");
        let mut files = SettingsFiles::load(folders.clone());
        drop(files.add_repository(&repository("a/first")).unwrap());
        drop(files.add_repository(&repository("a/second")).unwrap());

        let reopened = SettingsFiles::load(folders).home();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(reopened, opened("a/second", Conventions::Github));
    }

    #[test]
    fn a_last_repository_the_settings_no_longer_hold_gives_way_to_the_first() {
        let (folders, root) = temp_folders("reopen-gone");
        fs::write(
            folders.settings_file(),
            "[[repositories]]\nname = \"a/first\"\n[[repositories]]\nname = \"a/second\"\n",
        )
        .unwrap();
        fs::write(folders.machine_file(), "last_repository = \"a/gone\"\n").unwrap();

        let home = SettingsFiles::load(folders).home();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(home, opened("a/first", Conventions::Github));
    }

    #[test]
    fn the_last_repository_stays_on_this_machine_beside_what_the_user_wrote() {
        let (folders, root) = temp_folders("machine-last");
        fs::write(
            folders.machine_file(),
            "# This laptop.\nlast_repository = \"a/old\" # Mine.\neditor = \"code\"\n",
        )
        .unwrap();
        let mut files = SettingsFiles::load(folders.clone());

        drop(files.add_repository(&repository("a/new")).unwrap());
        let machine = fs::read_to_string(folders.machine_file()).unwrap();
        let settings = fs::read_to_string(folders.settings_file()).unwrap();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(
            machine,
            "# This laptop.\nlast_repository = \"a/new\" # Mine.\neditor = \"code\"\n"
        );
        assert!(!settings.contains(LAST_REPOSITORY), "{settings}");
    }

    #[test]
    fn adding_follows_settings_path() {
        let (folders, root) = temp_folders("add-moved");
        let moved = root.join("dotfiles.toml");
        point_at(&folders, &moved);
        let mut files = SettingsFiles::load(folders.clone());

        drop(files.add_repository(&repository("a/b")).unwrap());
        let text = fs::read_to_string(&moved).unwrap();
        let machine = fs::read_to_string(folders.machine_file()).unwrap();
        fs::remove_dir_all(root).unwrap();

        assert!(text.contains("a/b"), "{text}");
        assert!(machine.contains("settings_path"), "{machine}");
        assert!(machine.contains("last_repository = \"a/b\""), "{machine}");
    }

    #[test]
    fn adding_refuses_while_either_file_cant_be_used() {
        for (name, settings, machine, named) in [
            ("add-settings", "[status_bar\n", "", "settings.toml, line 1"),
            (
                "add-machine",
                "",
                "settings_path =\n",
                "machine.toml, line 1",
            ),
        ] {
            let (folders, root) = temp_folders(name);
            fs::write(folders.settings_file(), settings).unwrap();
            fs::write(folders.machine_file(), machine).unwrap();
            let mut files = SettingsFiles::load(folders.clone());

            let refused = files.add_repository(&repository("a/b")).unwrap_err();
            let after = fs::read_to_string(folders.settings_file()).unwrap();
            fs::remove_dir_all(root).unwrap();

            assert_eq!(after, settings);
            assert!(refused.message.contains(named), "{}", refused.message);
        }
    }

    #[test]
    #[expect(
        clippy::permissions_set_readonly_false,
        reason = "the test's own file, made writable again only so it can be removed"
    )]
    fn an_add_that_cant_save_machine_toml_says_so_and_adds_nothing() {
        let (folders, root) = temp_folders("add-machine-readonly");
        let file = folders.machine_file();
        fs::write(&file, "mine = 1\n").unwrap();
        let mut permissions = fs::metadata(&file).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&file, permissions.clone()).unwrap();
        let mut files = SettingsFiles::load(folders.clone());

        let refused = files.add_repository(&repository("a/b")).unwrap_err();
        let added = folders.settings_file().exists();
        permissions.set_readonly(false);
        fs::set_permissions(&file, permissions).unwrap();
        fs::remove_dir_all(root).unwrap();

        assert!(
            refused.message.contains("machine.toml"),
            "{}",
            refused.message
        );
        assert!(!added);
        assert_eq!(files.home(), Home::default());
    }

    #[test]
    fn a_settings_file_that_cant_be_read_refuses_the_add() {
        let (folders, root) = temp_folders("add-unreadable");
        fs::create_dir(folders.settings_file()).unwrap();
        let mut files = SettingsFiles::load(folders.clone());

        let refused = files.add_repository(&repository("a/b")).unwrap_err();
        fs::remove_dir_all(root).unwrap();

        assert!(
            refused.message.contains("settings.toml"),
            "{}",
            refused.message
        );
    }

    #[test]
    fn a_name_that_isnt_one_is_a_problem_named_with_its_line() {
        let (folders, root) = temp_folders("bad-name");
        fs::write(
            folders.settings_file(),
            "\n[[repositories]]\nname = \"nonsense\"\n",
        )
        .unwrap();
        let files = SettingsFiles::load(folders.clone());
        let in_settings = only_problem(&files);
        fs::write(folders.settings_file(), "").unwrap();
        fs::write(folders.machine_file(), "\nlast_repository = \"nonsense\"\n").unwrap();
        let files = SettingsFiles::load(folders);
        let in_machine = only_problem(&files);
        fs::remove_dir_all(root).unwrap();

        assert!(
            in_settings.contains("settings.toml, line 3"),
            "{in_settings}"
        );
        assert!(in_machine.contains("machine.toml, line 2"), "{in_machine}");
    }

    #[test]
    fn saving_settings_leaves_the_repositories_and_the_comments_on_them() {
        let before = "\
# Mine.
[[repositories]]
name = \"python/cpython\" # The big one.
";
        let (folders, root) = temp_folders("save-repositories");
        fs::write(folders.settings_file(), before).unwrap();
        let mut files = SettingsFiles::load(folders.clone());

        files.write(&HIDDEN).unwrap();
        let after = fs::read_to_string(folders.settings_file()).unwrap();
        let held = files.loaded().settings.repositories.len();
        fs::remove_dir_all(root).unwrap();

        assert!(after.starts_with(before), "{after}");
        assert!(after.ends_with("visible = false\n"), "{after}");
        assert_eq!(held, 1);
    }

    #[test]
    fn a_file_gone_from_settings_path_on_a_reload_keeps_the_last_good_values() {
        let (folders, root) = temp_folders("moved-gone");
        let moved = root.join("dotfiles.toml");
        fs::write(&moved, "[status_bar]\nvisible = false\n").unwrap();
        point_at(&folders, &moved);
        let mut files = SettingsFiles::load(folders);

        fs::remove_file(&moved).unwrap();
        files.reload();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(files.loaded().settings, HIDDEN);
        let problem = only_problem(&files);
        assert!(problem.contains("dotfiles.toml"), "{problem}");
        assert!(problem.contains("last values"), "{problem}");
    }
}
