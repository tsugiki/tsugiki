//! What the user configures, in `settings.toml`, which travels, and `machine.toml`, which stays (ADR 0014).
//!
//! Tsugiki reads both when it starts and when the Settings window opens, and an edit made in between waits
//! for one of those. That window's Apply saves what changed in it. A file that can't be used stays as it is,
//! `problems` says why, and the last good values stay.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use serde::de::{DeserializeOwned, IgnoredAny};
use serde::{Deserialize, Deserializer, Serialize};
use specta::Type;
use toml_edit::{ArrayOfTables, DocumentMut, Item, Key, RawString, Table, TableLike, Value};

use crate::error::{Error, ErrorKind};
use crate::folders::Folders;
use crate::latest::{self, LatestReader};
use crate::mirror::{MirrorSource, MirrorSources};
use crate::repository::{Home, RepositoryAccess, RepositoryName, RepositorySettings};

const REPOSITORIES: &str = "repositories";
const FILTERS: &str = "filters";
const LAST_REPOSITORY: &str = "last_repository";
const PANES: &str = "panes";
const MIRRORS: &str = "mirrors";
const STOPPED_SAVING: &str =
    "Tsugiki stopped saving its settings. Restart Tsugiki to save them again.";

/// What `settings.toml` holds. A value the file leaves out takes its default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct Settings {
    pub status_bar: StatusBarSettings,
    pub repositories: Vec<RepositorySettings>,
}

impl Settings {
    /// The settings of the Repository `name`, or a new Repository's if the user hasn't added it.
    pub fn repository(&self, name: RepositoryName) -> RepositorySettings {
        self.repositories
            .iter()
            .find(|repository| repository.name.is_same(&name))
            .cloned()
            .unwrap_or_else(|| RepositorySettings::new(name))
    }
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
    pub panes: PaneWidths,
    /// The user's answer, on this machine, of where each Repository's mirror gets its objects.
    pub mirror_sources: MirrorSources,
}

/// How wide the user dragged the Sidebar and the Workbench list, in CSS pixels (#187). Each stays on this
/// machine, like the window's place, and a pane left out takes its default. So does a width that isn't a
/// whole number of pixels: a pane's width is too small a thing to lose the rest of `machine.toml` over.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct PaneWidths {
    #[serde(deserialize_with = "width_or_default")]
    #[specta(type = Option<u32>)]
    pub sidebar: Option<u32>,
    #[serde(deserialize_with = "width_or_default")]
    #[specta(type = Option<u32>)]
    pub list: Option<u32>,
}

/// A pane's width, or nothing for one Tsugiki can't read.
fn width_or_default<'de, D: Deserializer<'de>>(given: D) -> Result<Option<u32>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Width {
        Pixels(u32),
        Other(IgnoredAny),
    }
    Ok(match Width::deserialize(given)? {
        Width::Pixels(pixels) => Some(pixels),
        Width::Other(_) => None,
    })
}

/// Each Repository's answer of where its mirror gets its objects, leaving out an entry Tsugiki can't read:
/// one answer is too small a thing to lose the rest of `machine.toml` over. Its Repository asks again.
fn sources_or_skip<'de, D: Deserializer<'de>>(given: D) -> Result<MirrorSources, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Answer {
        Text(String),
        Other(IgnoredAny),
    }
    let answers = BTreeMap::<String, Answer>::deserialize(given)?;
    let read = answers.into_iter().filter_map(|(name, answer)| {
        let name = RepositoryName::parse(&name).ok()?;
        match answer {
            Answer::Text(text) => Some((name, MirrorSource::from_text(&text))),
            Answer::Other(_) => None,
        }
    });
    Ok(MirrorSources::new(read.collect()))
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
    panes: PaneWidths,
    #[serde(deserialize_with = "sources_or_skip")]
    mirrors: MirrorSources,
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
            panes: self
                .machine
                .as_ref()
                .map_or_else(PaneWidths::default, |machine| machine.panes),
            mirror_sources: self
                .machine
                .as_ref()
                .map_or_else(MirrorSources::default, |machine| machine.mirrors.clone()),
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
    pub fn add_repository(
        &mut self,
        name: &RepositoryName,
        access: RepositoryAccess,
        source: &MirrorSource,
    ) -> Result<Home, Error> {
        let machine = self.machine_to_edit()?;
        let path = settings_file(&self.folders, &machine);
        let mut edit = TomlEdit::<Settings>::open(&path)?;
        let held = edit
            .values
            .repositories
            .iter()
            .any(|one| one.name.is_same(name));
        if !held {
            edit.append(&RepositorySettings::added(name.clone(), access));
        }
        remember_added(&self.folders.machine_file(), name, source)?;
        edit.save()?;
        self.reload();
        Ok(self.home())
    }

    /// Saves the panes' widths in `machine.toml`, keeping everything else in it, and reads the files again.
    /// It refuses while `machine.toml` can't be used.
    pub fn save_panes(&mut self, panes: PaneWidths) -> Result<(), Error> {
        let path = self.folders.machine_file();
        let mut edit = TomlEdit::<Machine>::open(&path)?;
        edit_section(edit.document.as_table_mut(), PANES, |section| {
            for (key, width) in [("sidebar", panes.sidebar), ("list", panes.list)] {
                // A reset takes the comment on its key too: kept, the key would hold the old width.
                match width {
                    Some(width) => {
                        merge_value(section, key, &toml_edit::value(i64::from(width)), None)
                    }
                    None => drop(section.remove(key)),
                }
            }
        });
        edit.save()?;
        self.reload();
        Ok(())
    }

    /// Saves where `name`'s mirror gets its objects in `machine.toml`, keeping everything else in it, and
    /// reads the files again.
    pub fn save_mirror_source(
        &mut self,
        name: &RepositoryName,
        source: &MirrorSource,
    ) -> Result<(), Error> {
        let path = self.folders.machine_file();
        let mut edit = TomlEdit::<Machine>::open(&path)?;
        set_mirror_source(edit.document.as_table_mut(), name, source);
        edit.save()?;
        self.reload();
        Ok(())
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
    requests: Sender<Request>,
    view: LatestReader<SettingsView>,
}

/// What the owner's thread saves, and where it answers with what the settings say after.
enum Request {
    Add {
        name: RepositoryName,
        access: RepositoryAccess,
        source: MirrorSource,
        reply: Sender<Result<SettingsView, Error>>,
    },
    MirrorSource {
        name: RepositoryName,
        source: MirrorSource,
        reply: Sender<Result<SettingsView, Error>>,
    },
    Panes {
        widths: PaneWidths,
        reply: Sender<Result<SettingsView, Error>>,
    },
}

impl SettingsOwner {
    /// From here on only this thread can read `files`, and `view` answers from its copy.
    pub fn start(mut files: Result<SettingsFiles, Error>) -> Self {
        let first = files.as_ref().map(SettingsFiles::view).unwrap_or_default();
        let (mut writer, view) = latest::channel(first);
        let (requests, incoming) = mpsc::channel::<Request>();
        let _ = thread::spawn(move || {
            for request in incoming {
                let (saved, reply) = match (&mut files, request) {
                    (
                        Ok(files),
                        Request::Add {
                            name,
                            access,
                            source,
                            reply,
                        },
                    ) => {
                        let saved = files.add_repository(&name, access, &source);
                        (saved.map(|_| files.view()), reply)
                    }
                    (
                        Ok(files),
                        Request::MirrorSource {
                            name,
                            source,
                            reply,
                        },
                    ) => {
                        let saved = files.save_mirror_source(&name, &source);
                        (saved.map(|()| files.view()), reply)
                    }
                    (Ok(files), Request::Panes { widths, reply }) => {
                        (files.save_panes(widths).map(|()| files.view()), reply)
                    }
                    (
                        Err(problem),
                        Request::Add { reply, .. }
                        | Request::MirrorSource { reply, .. }
                        | Request::Panes { reply, .. },
                    ) => (Err(problem.clone()), reply),
                };
                // Before the answer, so whoever asked finds the change in `view`. Changes come through here
                // one at a time, so an older one can't replace a newer one.
                if let Ok(saved) = &saved {
                    writer.set(saved);
                }
                #[expect(
                    clippy::let_underscore_must_use,
                    reason = "this fails only if the caller has gone, and it waits for this answer"
                )]
                let _ = reply.send(saved);
            }
        });
        Self { requests, view }
    }

    /// What the settings said after the last change saved. This never waits on a file.
    pub fn view(&self) -> SettingsView {
        self.view.get()
    }

    /// Adds `name` to the settings, and answers with what they say now.
    pub fn add_repository(
        &self,
        name: RepositoryName,
        access: RepositoryAccess,
        source: MirrorSource,
    ) -> Result<SettingsView, Error> {
        let (reply, answer) = mpsc::channel();
        self.ask(
            Request::Add {
                name,
                access,
                source,
                reply,
            },
            &answer,
        )
    }

    /// Saves where `name`'s mirror gets its objects in `machine.toml`, and answers with what the settings say
    /// now.
    pub fn save_mirror_source(
        &self,
        name: RepositoryName,
        source: MirrorSource,
    ) -> Result<SettingsView, Error> {
        let (reply, answer) = mpsc::channel();
        self.ask(
            Request::MirrorSource {
                name,
                source,
                reply,
            },
            &answer,
        )
    }

    /// Saves the panes' widths in `machine.toml`, and answers with what the settings say now.
    pub fn save_panes(&self, widths: PaneWidths) -> Result<SettingsView, Error> {
        let (reply, answer) = mpsc::channel();
        self.ask(Request::Panes { widths, reply }, &answer)
    }

    fn ask(
        &self,
        request: Request,
        answer: &Receiver<Result<SettingsView, Error>>,
    ) -> Result<SettingsView, Error> {
        self.requests
            .send(request)
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
            tables.push(with_filters_as_tables(table));
        } else if let Some(written_inline) = repositories.as_array_mut() {
            written_inline.push(table.into_inline_table());
        }
    }
}

/// A Filter takes a block of its own, because the user edits these by hand (ADR 0014).
fn with_filters_as_tables(mut repository: Table) -> Table {
    if let Some(filters) = repository.remove(FILTERS) {
        // An empty list can't become tables, and it stays: `filters = []` means no Filters.
        let filters = filters
            .into_array_of_tables()
            .map_or_else(|kept| kept, Item::ArrayOfTables);
        drop(repository.insert(FILTERS, filters));
    }
    repository
}

/// `write` saves what the Settings window changes, and that window doesn't edit the Repository list. Merging
/// the list would replace the file's tables, and the user's comments on them.
fn without_repositories(mut document: DocumentMut) -> DocumentMut {
    drop(document.remove(REPOSITORIES));
    document
}

/// Notes in `machine.toml` that `name` was added last, and where its mirror gets its objects.
fn remember_added(path: &Path, name: &RepositoryName, source: &MirrorSource) -> Result<(), Error> {
    let mut edit = TomlEdit::<Machine>::open(path)?;
    let wanted = toml_edit::value(name.to_string());
    merge_value(edit.document.as_table_mut(), LAST_REPOSITORY, &wanted, None);
    set_mirror_source(edit.document.as_table_mut(), name, source);
    edit.save()
}

/// Makes `file`'s `[mirrors]` hold `source` for `name`, under one key: one written in another case goes.
fn set_mirror_source(file: &mut dyn TableLike, name: &RepositoryName, source: &MirrorSource) {
    let key = name.to_string();
    edit_section(file, MIRRORS, |section| {
        let others: Vec<String> = section
            .iter()
            .map(|(one, _)| one.to_owned())
            .filter(|one| {
                *one != key && RepositoryName::parse(one).is_ok_and(|one| one.is_same(name))
            })
            .collect();
        for other in others {
            drop(section.remove(&other));
        }
        merge_value(section, &key, &toml_edit::value(source.to_text()), None);
    });
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
    edit_section(file, key, |section| merge(section, wanted, defaults));
}

/// Hands `change` the section `key` of `file`, made first if `file` has none, and drops the section again
/// if `change` leaves it empty.
fn edit_section(file: &mut dyn TableLike, key: &str, change: impl FnOnce(&mut dyn TableLike)) {
    if file.get(key).is_none() {
        drop(file.insert(key, Item::Table(Table::new())));
    }
    if let Some(section) = file.get_mut(key).and_then(Item::as_table_like_mut) {
        change(section);
    }
    drop_if_empty(file, key);
}

/// A section with nothing left in it goes, unless the user wrote a comment on it.
fn drop_if_empty(file: &mut dyn TableLike, key: &str) {
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
mod tests;
