//! What the user configures, in `settings.toml`, which travels, and `machine.toml`, which stays (ADR 0014).
//!
//! Tsugiki reads both when it starts and when the Settings window opens, and an edit made in between waits
//! for one of those. That window's Apply saves what changed in it. A file that can't be used stays as it is,
//! `problems` says why, and the last good values stay.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use serde::de::{DeserializeOwned, IgnoredAny};
use serde::{Deserialize, Deserializer, Serialize};
use specta::Type;
use time::{OffsetDateTime, UtcOffset};
use toml_edit::{
    Array, ArrayOfTables, Datetime, DocumentMut, InlineTable, Item, Key, RawString, Table,
    TableLike, Value,
};

use crate::error::{Error, ErrorKind};
use crate::folders::Folders;
use crate::latest::{self, LatestReader};
use crate::mirror::{MirrorSource, MirrorSources};
use crate::repository::{RepositoryAccess, RepositoryName};
use crate::repository_settings::{NameCheck, RepositoryCheck, RepositorySettings};
use crate::theme::{ThemeKind, ThemeMode, ThemeName};

const REPOSITORIES: &str = "repositories";
const FILTERS: &str = "filters";
const RECENT: &str = "recent";
/// The keys of an entry in `recent`.
const NAME: &str = "name";
const OPENED_AT: &str = "opened_at";
const LAST_REPOSITORY: &str = "last_repository";
const PANES: &str = "panes";
const APPEARANCE: &str = "appearance";
const DIFF: &str = "diff";
const MIRRORS: &str = "mirrors";
const STOPPED_SAVING: &str =
    "Tsugiki stopped saving its settings. Restart Tsugiki to save them again.";

/// What `settings.toml` holds. A value the file leaves out takes its default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct Settings {
    pub status_bar: StatusBarSettings,
    pub releases: ReleaseSettings,
    pub appearance: AppearanceSettings,
    pub diff: DiffSettings,
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

    /// What Add repository says about `name`. A name these settings hold, in any case, is held by the name
    /// they hold it by, and GitHub isn't asked. Otherwise `ask_github` checks it, and the name GitHub answers
    /// with can still be one these settings hold: GitHub follows a renamed Repository to its new name.
    ///
    /// `ask_github` isn't generic, so the app and the tests run one copy of this, and the coverage gate sees
    /// each line run.
    pub fn check_name(
        &self,
        name: RepositoryName,
        ask_github: &mut dyn FnMut(RepositoryName) -> Result<RepositoryCheck, Error>,
    ) -> Result<NameCheck, Error> {
        if let Some(name) = self.held(&name) {
            return Ok(NameCheck::Held { name });
        }
        let check = ask_github(name)?;
        Ok(match self.held(&check.repository.name) {
            Some(name) => NameCheck::Held { name },
            None => NameCheck::New { check },
        })
    }

    /// The name these settings hold `name` by, if they hold it.
    fn held(&self, name: &RepositoryName) -> Option<RepositoryName> {
        self.repositories
            .iter()
            .find(|repository| repository.name.is_same(name))
            .map(|repository| repository.name.clone())
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

/// The `[releases]` table: whether Tsugiki asks GitHub Releases for a newer version (ADR 0012).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct ReleaseSettings {
    /// Off, Tsugiki sends no request to Releases. A change takes effect at the next start.
    pub check: bool,
}

impl Default for ReleaseSettings {
    fn default() -> Self {
        Self { check: true }
    }
}

/// The `[appearance]` table: whether the window follows the system to its light or dark side, and the theme
/// of each side (#517). A key Tsugiki can't read takes its default, and `unread_problems` says why: a
/// theme is too small a thing to lose the rest of `settings.toml` over. An `appearance` that isn't a table
/// fails the file, as any other table does.
///
/// A theme's name may be a built-in theme's or a file's of the `themes` folder beside the settings file
/// (#519). It stays as the user wrote it: the folder can change while Tsugiki runs, so the `themes` command
/// checks it against the folder each time, and a name no theme has shows its side's default until one does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct AppearanceSettings {
    pub theme: ThemeMode,
    pub light_theme: ThemeName,
    pub dark_theme: ThemeName,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::default(),
            light_theme: ThemeName::default_of(ThemeKind::Light),
            dark_theme: ThemeName::default_of(ThemeKind::Dark),
        }
    }
}

impl<'de> Deserialize<'de> for AppearanceSettings {
    fn deserialize<D: Deserializer<'de>>(given: D) -> Result<Self, D::Error> {
        Ok(AppearanceRead::deserialize(given)?.settle().0)
    }
}

/// `[appearance]` as the file gives it, before a value Tsugiki can't read takes its default.
#[derive(Default, Deserialize)]
#[serde(default)]
struct AppearanceRead {
    theme: Option<Read<ThemeMode>>,
    light_theme: Option<Read<ThemeName>>,
    dark_theme: Option<Read<ThemeName>>,
}

/// A key of `[appearance]` given a value Tsugiki can't read, and what Tsugiki does instead, in words for the
/// status bar.
type Unread = (&'static str, String);

impl AppearanceRead {
    /// The values, with each one Tsugiki can't read at its default, and the keys that held those.
    fn settle(self) -> (AppearanceSettings, Vec<Unread>) {
        let mut unread = Vec::new();
        let theme = given_or_default(self.theme, "theme", &mut unread, || {
            r#"isn't "system", "light" or "dark", so Tsugiki follows the system."#.to_owned()
        });
        let mut side = |given: Option<Read<ThemeName>>, key: &'static str, kind: ThemeKind| {
            let default = ThemeName::default_of(kind);
            match given {
                None => default,
                Some(Read::Given(name)) => name,
                Some(Read::Other(_)) => {
                    let instead = default.as_str();
                    let problem = format!("isn't a theme's name, so Tsugiki uses {instead}.");
                    unread.push((key, problem));
                    default
                }
            }
        };
        let light_theme = side(self.light_theme, "light_theme", ThemeKind::Light);
        let dark_theme = side(self.dark_theme, "dark_theme", ThemeKind::Dark);
        let settings = AppearanceSettings {
            theme,
            light_theme,
            dark_theme,
        };
        (settings, unread)
    }
}

/// The value the file gave `key`, or its default. One Tsugiki can't read joins `unread`, with `instead`.
fn given_or_default<T: Default>(
    given: Option<Read<T>>,
    key: &'static str,
    unread: &mut Vec<Unread>,
    instead: impl FnOnce() -> String,
) -> T {
    match given {
        None => T::default(),
        Some(Read::Given(value)) => value,
        Some(Read::Other(_)) => {
            unread.push((key, instead()));
            T::default()
        }
    }
}

/// The `[diff]` table: how the Files tab shows a PR's diff (#605). A `files` Tsugiki can't read takes its
/// default, and `unread_problems` says why, as for `[appearance]`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Type)]
pub struct DiffSettings {
    pub files: DiffFiles,
}

/// Whether a PR opens on all its files, one under the other, or on one file at a time: `[diff] files`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DiffFiles {
    #[default]
    All,
    One,
}

impl<'de> Deserialize<'de> for DiffSettings {
    fn deserialize<D: Deserializer<'de>>(given: D) -> Result<Self, D::Error> {
        Ok(DiffRead::deserialize(given)?.settle().0)
    }
}

/// `[diff]` as the file gives it, before a value Tsugiki can't read takes its default.
#[derive(Default, Deserialize)]
#[serde(default)]
struct DiffRead {
    files: Option<Read<DiffFiles>>,
}

impl DiffRead {
    /// The values, with each one Tsugiki can't read at its default, and the keys that held those.
    fn settle(self) -> (DiffSettings, Vec<Unread>) {
        let mut unread = Vec::new();
        let settings = DiffSettings {
            files: given_or_default(self.files, "files", &mut unread, || {
                r#"isn't "all" or "one", so each PR opens on all its files."#.to_owned()
            }),
        };
        (settings, unread)
    }
}

/// The problems with the `[appearance]` and `[diff]` of the settings file `text`, from `path`, in words for
/// the status bar. Each names its line, when the parser knows it.
fn unread_problems(path: &Path, text: &str) -> Vec<String> {
    /// Only the tables of the file whose values fall back to their defaults.
    #[derive(Default, Deserialize)]
    #[serde(default)]
    struct File {
        appearance: AppearanceRead,
        diff: DiffRead,
    }
    // Each caller has read the whole file by now, so this read finds what that one did.
    let file = toml_edit::de::from_str::<File>(text).unwrap_or_default();
    let document = toml_edit::Document::parse(text).ok();
    [
        (APPEARANCE, file.appearance.settle().1),
        (DIFF, file.diff.settle().1),
    ]
    .into_iter()
    .flat_map(|(name, unread)| {
        let table = document
            .as_ref()
            .and_then(|document| document.get(name))
            .and_then(Item::as_table_like);
        unread.into_iter().map(move |(key, problem)| {
            let line = table
                .and_then(|table| table.get_key_value(key))
                .and_then(|(key, _)| key.span())
                .map_or_else(String::new, |span| line_of(text, span.start));
            format!("{}{line}: {key} {problem}", path.display())
        })
    })
    .collect()
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
    pub panes: PaneLayout,
    /// The user's answer, on this machine, of where each Repository's mirror gets its objects.
    pub mirror_sources: MirrorSources,
    /// The folder of the user's theme files, beside the settings file in use (#519).
    pub themes_folder: PathBuf,
    pub language_servers: LanguageServers,
}

/// Where the language servers the user supplies are on this machine (ADR 0020), each from its
/// `[language_servers.NAME] path` in `machine.toml`. A relative path starts at `machine.toml`'s folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageServers {
    /// ty, which Go to definition asks in Python.
    pub ty: Option<PathBuf>,
    /// rust-analyzer, which Go to definition asks in Rust.
    pub rust_analyzer: Option<PathBuf>,
    /// clangd, which Go to definition asks in C.
    pub clangd: Option<PathBuf>,
    /// The idle time, from `[language_servers] idle_minutes`: a server that no lookup reaches for this long
    /// stops.
    pub idle: Duration,
}

/// The idle time, in minutes, unless `machine.toml` says otherwise.
const IDLE_MINUTES: NonZeroU32 = NonZeroU32::new(30).unwrap();

impl Default for LanguageServers {
    fn default() -> Self {
        Self {
            ty: None,
            rust_analyzer: None,
            clangd: None,
            idle: minutes(IDLE_MINUTES),
        }
    }
}

fn minutes(count: NonZeroU32) -> Duration {
    Duration::from_secs(u64::from(count.get()) * 60)
}

/// Where the token comes from. A personal access token joins this with the Account page (ADR 0004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TokenSource {
    Gh,
}

/// What the window opens on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "screen", rename_all = "snake_case")]
pub enum Home {
    /// No Repositories yet.
    Onboarding { token: TokenSource },
    /// The Repository opened most recently on this machine that the settings still hold, or else the first.
    Repository {
        repository: RepositorySettings,
        /// The names of the other Repositories the settings hold, in the order the Repository picker lists
        /// them below the open one: those opened on this machine first, most recently opened first, and then
        /// the rest in the settings' order.
        others: Vec<RepositoryName>,
    },
}

impl Default for Home {
    fn default() -> Self {
        Self::Onboarding {
            token: TokenSource::Gh,
        }
    }
}

/// How the user laid out the panes: how wide they dragged the Sidebar and the Workbench list, in CSS pixels
/// (#187), whether the Sidebar (#234) and the list (#242) show, which of them `Ctrl+K Z` hid (#246), to
/// bring back on the next press, the list's order (#592), and how many rows tall they dragged the Definition
/// peek (#458). All of it stays on this machine, like the window's place, and a value left out takes its
/// default. So does a value Tsugiki can't read, such as a width that isn't a whole number of pixels: a pane
/// is too small a thing to lose the rest of `machine.toml` over.
#[expect(
    clippy::struct_excessive_bools,
    reason = "Each pane hides by itself, so all four states are valid: both hidden leaves the Detail pane"
)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct PaneLayout {
    #[serde(deserialize_with = "or_default")]
    #[specta(type = Option<u32>)]
    pub sidebar: Option<u32>,
    #[serde(deserialize_with = "or_default")]
    #[specta(type = Option<u32>)]
    pub list: Option<u32>,
    /// The Definition peek's height in diff rows, its header among them, or nothing for its 18.
    #[serde(deserialize_with = "or_default")]
    #[specta(type = Option<u32>)]
    pub peek: Option<u32>,
    /// `Ctrl+B` hid the Sidebar. It keeps its width for when it shows again.
    #[serde(deserialize_with = "or_default")]
    #[specta(type = bool)]
    pub sidebar_hidden: bool,
    /// `Ctrl+Alt+B` hid the Workbench list. It keeps its width for when it shows again.
    #[serde(deserialize_with = "or_default")]
    #[specta(type = bool)]
    pub list_hidden: bool,
    /// The panes `Ctrl+K Z` hid to show only the Detail pane, which its next press brings back. Nothing
    /// while it remembers none: before the first press, and after `Ctrl+B`, `Ctrl+Alt+B` or "Reset Layout"
    /// made it forget.
    #[serde(deserialize_with = "or_default")]
    #[specta(type = Option<Panes>)]
    pub detail_only_hid: Option<Panes>,
    /// How the Issue list orders its items (#592).
    #[serde(deserialize_with = "or_default")]
    #[specta(type = ListOrder)]
    pub order: ListOrder,
}

/// How the Issue list orders its items, apart from its quiet section, which comes last either way (#592).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ListOrder {
    /// What Needs attention first, newest first by when it started to, then the rest by last update.
    #[default]
    Attention,
    /// Newest first by last update.
    LastUpdate,
}

impl ListOrder {
    /// The name `machine.toml` gives it, as serde reads it.
    fn to_text(self) -> &'static str {
        match self {
            Self::Attention => "attention",
            Self::LastUpdate => "last_update",
        }
    }
}

/// The Sidebar, the Workbench list, or both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Panes {
    Sidebar,
    List,
    Both,
}

impl Panes {
    /// The name `machine.toml` gives it, as serde reads it.
    fn to_text(self) -> &'static str {
        match self {
            Self::Sidebar => "sidebar",
            Self::List => "list",
            Self::Both => "both",
        }
    }
}

/// A value of the type asked for, or anything else.
#[derive(Deserialize)]
#[serde(untagged)]
enum Read<T> {
    Given(T),
    Other(IgnoredAny),
}

impl<T> Read<T> {
    fn given(self) -> Option<T> {
        match self {
            Self::Given(value) => Some(value),
            Self::Other(_) => None,
        }
    }
}

/// The value, or its default for one Tsugiki can't read.
fn or_default<'de, D: Deserializer<'de>, T: Deserialize<'de> + Default>(
    given: D,
) -> Result<T, D::Error> {
    Ok(Read::<T>::deserialize(given)?.given().unwrap_or_default())
}

/// The names in `recent`, leaving out an entry Tsugiki can't read, for the reason `sources_or_skip` gives.
/// A `recent` that isn't a list counts as none. Only the order is read: nothing asks when yet.
fn recent_or_skip<'de, D: Deserializer<'de>>(
    given: D,
) -> Result<Option<Vec<RepositoryName>>, D::Error> {
    /// An entry of `recent`. Its field is the `NAME` that `recent_entry` writes.
    #[derive(Deserialize)]
    struct Opened {
        name: RepositoryName,
    }
    let entries = Read::<Vec<Read<Opened>>>::deserialize(given)?.given();
    Ok(entries.map(|entries| {
        entries
            .into_iter()
            .filter_map(|entry| Some(entry.given()?.name))
            .collect()
    }))
}

/// Each Repository's answer of where its mirror gets its objects, leaving out an entry Tsugiki can't read:
/// one answer is too small a thing to lose the rest of `machine.toml` over. Its Repository asks again.
fn sources_or_skip<'de, D: Deserializer<'de>>(given: D) -> Result<MirrorSources, D::Error> {
    let answers = BTreeMap::<String, Read<String>>::deserialize(given)?;
    let read = answers.into_iter().filter_map(|(name, answer)| {
        let name = RepositoryName::parse(&name).ok()?;
        Some((name, MirrorSource::from_text(&answer.given()?)))
    });
    Ok(MirrorSources::new(read.collect()))
}

/// `machine.toml` as serde reads it, before `Machine` settles which list of recent Repositories counts.
#[derive(Default, Deserialize)]
#[serde(default)]
struct MachineFile {
    settings_path: Option<PathBuf>,
    #[serde(deserialize_with = "recent_or_skip")]
    recent: Option<Vec<RepositoryName>>,
    /// The Repository to reopen, as 0.2.0 kept it. It counts while the file has no `recent`, and the next
    /// write of `machine.toml` puts `recent` in its place.
    last_repository: Option<RepositoryName>,
    panes: PaneLayout,
    #[serde(deserialize_with = "sources_or_skip")]
    mirrors: MirrorSources,
    #[serde(deserialize_with = "or_default")]
    language_servers: ServersFile,
}

/// `machine.toml`'s `[language_servers]`: a table for each server, which holds its `path`, and how long an
/// idle server runs. A server whose table Tsugiki can't read counts as not set up, and an `idle_minutes`
/// that isn't a whole number above 0 counts as unset.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct ServersFile {
    #[serde(deserialize_with = "or_default")]
    idle_minutes: Option<NonZeroU32>,
    #[serde(deserialize_with = "or_default")]
    ty: Option<ServerFile>,
    #[serde(rename = "rust-analyzer", deserialize_with = "or_default")]
    rust_analyzer: Option<ServerFile>,
    #[serde(deserialize_with = "or_default")]
    clangd: Option<ServerFile>,
}

#[derive(Debug, Clone, Deserialize)]
struct ServerFile {
    path: PathBuf,
}

/// What `machine.toml` holds, as far as this file needs it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(from = "MachineFile")]
struct Machine {
    /// Where `settings.toml` is, if not in its usual folder. A relative path starts at `machine.toml`'s
    /// folder.
    settings_path: Option<PathBuf>,
    /// The Repositories opened most recently on this machine, newest first. They stay on this machine and
    /// are read with the settings, so a normal launch knows what to reopen before the window exists.
    recent: Vec<RepositoryName>,
    panes: PaneLayout,
    mirrors: MirrorSources,
    /// Each language server's path as the file gives it, which may be relative.
    servers: ServersFile,
}

impl From<MachineFile> for Machine {
    fn from(file: MachineFile) -> Self {
        Self {
            settings_path: file.settings_path,
            recent: file
                .recent
                .unwrap_or_else(|| file.last_repository.into_iter().collect()),
            panes: file.panes,
            mirrors: file.mirrors,
            servers: file.language_servers,
        }
    }
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
                .map_or_else(PaneLayout::default, |machine| machine.panes),
            mirror_sources: self
                .machine
                .as_ref()
                .map_or_else(MirrorSources::default, |machine| machine.mirrors.clone()),
            themes_folder: themes_folder(&settings_file(
                &self.folders,
                self.machine.as_ref().unwrap_or(&Machine::default()),
            )),
            language_servers: {
                let path = |server: fn(&ServersFile) -> Option<&ServerFile>| {
                    (self.machine.as_ref())
                        .and_then(|machine| server(&machine.servers))
                        .map(|file| self.folders.data.join(&file.path))
                };
                LanguageServers {
                    ty: path(|servers| servers.ty.as_ref()),
                    rust_analyzer: path(|servers| servers.rust_analyzer.as_ref()),
                    clangd: path(|servers| servers.clangd.as_ref()),
                    idle: minutes(
                        (self.machine.as_ref())
                            .and_then(|machine| machine.servers.idle_minutes)
                            .unwrap_or(IDLE_MINUTES),
                    ),
                }
            },
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
                Ok((settings, unread)) => {
                    self.loaded.settings = settings;
                    problems.extend(unread);
                }
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
    /// `settings.toml` might hold work the user hasn't finished. So does an `[appearance]` or `[diff]` value
    /// Tsugiki can't read.
    pub fn write(&mut self, settings: &Settings) -> Result<(), Error> {
        let machine = self.machine_to_edit()?;
        let path = settings_file(&self.folders, &machine);
        let mut edit = TomlEdit::<Settings>::open(&path)?;
        // A key Tsugiki read as its default would be saved as the default, over what the user wrote.
        if let Some(problem) = unread_problems(&path, &edit.before).first() {
            return Err(refused(problem));
        }
        merge(
            edit.document.as_table_mut(),
            without_repositories(as_document(settings)).as_table(),
            without_repositories(as_document(&Settings::default())).as_table(),
        );
        edit.save()?;
        self.reload();
        Ok(())
    }

    /// What the window opens on: the Repository opened most recently that the settings still hold, or else
    /// the first. The others follow in the Repository picker's order: those in `recent` as it lists them,
    /// then the rest in the settings' order, each name once.
    pub fn home(&self) -> Home {
        let repositories = &self.loaded.settings.repositories;
        let recent = self
            .machine
            .as_ref()
            .map_or(&[][..], |machine| &machine.recent);
        let reopened = recent
            .iter()
            .filter_map(|name| repositories.iter().find(|one| one.name.is_same(name)));
        // A name the user wrote twice, in either file, comes once.
        let mut order: Vec<&RepositorySettings> = Vec::with_capacity(repositories.len());
        for repository in reopened.chain(repositories) {
            if !order.iter().any(|one| one.name.is_same(&repository.name)) {
                order.push(repository);
            }
        }
        match order.split_first() {
            Some((&repository, others)) => Home::Repository {
                repository: repository.clone(),
                others: others.iter().map(|one| one.name.clone()).collect(),
            },
            None => Home::default(),
        }
    }

    /// Adds `repository` to the settings, unless they hold its name already, and puts it at the head of
    /// `recent`, opened at `opened_at`, so the window opens on it.
    ///
    /// Like `write`, it works from what the files say now and refuses while either can't be used. It saves
    /// `machine.toml` first: if that fails, nothing has changed. If `settings.toml` then fails, the name
    /// there holds nothing, and `home` passes over it.
    pub fn add_repository(
        &mut self,
        repository: &RepositorySettings,
        source: &MirrorSource,
        opened_at: OffsetDateTime,
    ) -> Result<Home, Error> {
        let name = &repository.name;
        let machine = self.machine_to_edit()?;
        let path = settings_file(&self.folders, &machine);
        let mut edit = TomlEdit::<Settings>::open(&path)?;
        let held = edit
            .values
            .repositories
            .iter()
            .any(|one| one.name.is_same(name));
        if !held {
            edit.append(repository);
        }
        remember_added(&self.folders.machine_file(), name, source, opened_at)?;
        edit.save()?;
        self.reload();
        Ok(self.home())
    }

    /// Makes `name` the open Repository: puts it at the head of `recent`, opened at `opened_at`, under the
    /// name the settings hold it by. Nothing else in either file changes, its mirror answer included.
    ///
    /// Like `add_repository`, it works from what the files say now. It refuses a name the settings don't
    /// hold, and refuses while `machine.toml` can't be used.
    pub fn open_repository(
        &mut self,
        name: &RepositoryName,
        opened_at: OffsetDateTime,
    ) -> Result<Home, Error> {
        self.reload();
        let held = self.loaded.settings.held(name).ok_or_else(|| {
            Error::new(
                ErrorKind::NotFound,
                &format!("Tsugiki doesn't have {name}."),
            )
        })?;
        let path = self.folders.machine_file();
        let mut edit = TomlEdit::open_machine(&path)?;
        put_first_in_recent(edit.document.as_table_mut(), &held, opened_at);
        edit.save()?;
        self.reload();
        Ok(self.home())
    }

    /// Saves the panes' layout in `machine.toml`, keeping everything else in it, and reads the files again.
    /// It refuses while `machine.toml` can't be used.
    pub fn save_panes(&mut self, panes: PaneLayout) -> Result<(), Error> {
        let path = self.folders.machine_file();
        let mut edit = TomlEdit::open_machine(&path)?;
        edit_section(edit.document.as_table_mut(), PANES, |section| {
            for (key, size) in [
                ("sidebar", panes.sidebar),
                ("list", panes.list),
                ("peek", panes.peek),
            ] {
                // A reset takes the comment on its key too: kept, the key would hold the old size.
                match size {
                    Some(size) => {
                        merge_value(section, key, &toml_edit::value(i64::from(size)), None)
                    }
                    None => drop(section.remove(key)),
                }
            }
            for (key, hidden) in [
                ("sidebar_hidden", panes.sidebar_hidden),
                ("list_hidden", panes.list_hidden),
            ] {
                merge_value(
                    section,
                    key,
                    &toml_edit::value(hidden),
                    Some(&toml_edit::value(false)),
                );
            }
            let key = "detail_only_hid";
            match panes.detail_only_hid {
                Some(hid) => merge_value(section, key, &toml_edit::value(hid.to_text()), None),
                None => drop(section.remove(key)),
            }
            merge_value(
                section,
                "order",
                &toml_edit::value(panes.order.to_text()),
                Some(&toml_edit::value(ListOrder::default().to_text())),
            );
        });
        edit.save()?;
        self.reload();
        Ok(())
    }

    /// Saves the theme the picker chose for its side to `light_theme` or `dark_theme`, keeping everything
    /// else in `settings.toml`, and reads the files again (#518). Like `write`, it works from what the files
    /// say now and refuses while either can't be used.
    pub fn save_theme(&mut self, kind: ThemeKind, name: &ThemeName) -> Result<(), Error> {
        let machine = self.machine_to_edit()?;
        let path = settings_file(&self.folders, &machine);
        let mut edit = TomlEdit::<Settings>::open(&path)?;
        let key = match kind {
            ThemeKind::Light => "light_theme",
            ThemeKind::Dark => "dark_theme",
        };
        let default = ThemeName::default_of(kind);
        edit_section(edit.document.as_table_mut(), APPEARANCE, |section| {
            merge_value(
                section,
                key,
                &toml_edit::value(name.as_str()),
                Some(&toml_edit::value(default.as_str())),
            );
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
        let mut edit = TomlEdit::open_machine(&path)?;
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
        layout: PaneLayout,
        reply: Sender<Result<SettingsView, Error>>,
    },
    Theme {
        kind: ThemeKind,
        name: ThemeName,
        reply: Sender<Result<SettingsView, Error>>,
    },
    Open {
        name: RepositoryName,
        reply: Sender<Result<SettingsView, Error>>,
    },
    Reread {
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
                        let saved = files.add_repository(
                            &RepositorySettings::added(name, access),
                            &source,
                            OffsetDateTime::now_utc(),
                        );
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
                    (Ok(files), Request::Panes { layout, reply }) => {
                        (files.save_panes(layout).map(|()| files.view()), reply)
                    }
                    (Ok(files), Request::Theme { kind, name, reply }) => {
                        (files.save_theme(kind, &name).map(|()| files.view()), reply)
                    }
                    (Ok(files), Request::Open { name, reply }) => {
                        let opened = files.open_repository(&name, OffsetDateTime::now_utc());
                        (opened.map(|_| files.view()), reply)
                    }
                    (Ok(files), Request::Reread { reply }) => {
                        files.reload();
                        (Ok(files.view()), reply)
                    }
                    (
                        Err(problem),
                        Request::Add { reply, .. }
                        | Request::MirrorSource { reply, .. }
                        | Request::Panes { reply, .. }
                        | Request::Theme { reply, .. }
                        | Request::Open { reply, .. }
                        | Request::Reread { reply },
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

    /// A copy of what the settings say, for a thread that reads them later.
    pub fn reader(&self) -> LatestReader<SettingsView> {
        self.view.clone()
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

    /// Saves the panes' layout in `machine.toml`, and answers with what the settings say now.
    pub fn save_panes(&self, layout: PaneLayout) -> Result<SettingsView, Error> {
        let (reply, answer) = mpsc::channel();
        self.ask(Request::Panes { layout, reply }, &answer)
    }

    /// Saves the theme the picker chose for its side in `settings.toml`, and answers with what the settings
    /// say now.
    pub fn save_theme(&self, kind: ThemeKind, name: ThemeName) -> Result<SettingsView, Error> {
        let (reply, answer) = mpsc::channel();
        self.ask(Request::Theme { kind, name, reply }, &answer)
    }

    /// Makes `name` the open Repository, and answers with what the settings say now.
    pub fn open_repository(&self, name: RepositoryName) -> Result<SettingsView, Error> {
        let (reply, answer) = mpsc::channel();
        self.ask(Request::Open { name, reply }, &answer)
    }

    /// What Add repository says about `name` (`Settings::check_name`), from what the files say now: a
    /// Repository the user took out of `settings.toml` by hand is no longer held. Without files to read it
    /// goes by the view. GitHub is asked on this thread, so the owner never waits for it.
    pub fn check_name(
        &self,
        name: RepositoryName,
        ask_github: &mut dyn FnMut(RepositoryName) -> Result<RepositoryCheck, Error>,
    ) -> Result<NameCheck, Error> {
        let (reply, answer) = mpsc::channel();
        let view = self
            .ask(Request::Reread { reply }, &answer)
            .unwrap_or_else(|_| self.view());
        view.loaded.settings.check_name(name, ask_github)
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

impl<'a> TomlEdit<'a, Machine> {
    /// `machine.toml` as it says now, open for changes, with its list of recent Repositories brought to the
    /// form this version writes, so that any write leaves the file in that form.
    fn open_machine(path: &'a Path) -> Result<Self, Error> {
        let mut edit = Self::open(path)?;
        tidy_recent(edit.document.as_table_mut());
        Ok(edit)
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

/// Notes in `machine.toml` that `name` was opened at `opened_at`, and where its mirror gets its objects.
fn remember_added(
    path: &Path,
    name: &RepositoryName,
    source: &MirrorSource,
    opened_at: OffsetDateTime,
) -> Result<(), Error> {
    let mut edit = TomlEdit::open_machine(path)?;
    put_first_in_recent(edit.document.as_table_mut(), name, opened_at);
    set_mirror_source(edit.document.as_table_mut(), name, source);
    edit.save()
}

/// Brings `file`'s list of recent Repositories to the form this version writes: one list, written inline.
///
/// A list of `[[recent]]` tables, written by hand, comes inline with its entries, though not the comments on
/// their headers. A 0.2.0 `last_repository` becomes a list of its Repository, with no time, since 0.2.0
/// kept none, where it stood and with its comments, unless the file has a list already. Such a file ran
/// this version, then 0.2.0, and the list counts, as it does for a launch.
fn tidy_recent(file: &mut Table) {
    if file.get(RECENT).is_some_and(Item::is_array_of_tables)
        && let Some(tables) = file.remove(RECENT)
        && let Ok(list) = tables.into_value()
    {
        drop(file.insert(RECENT, Item::Value(list)));
    }
    let has_list = file.get(RECENT).is_some_and(Item::is_array);
    let order: Vec<String> = file
        .iter()
        .filter(|(key, _)| *key != RECENT || has_list)
        .map(|(key, _)| if key == LAST_REPOSITORY { RECENT } else { key }.to_owned())
        .collect();
    let Some((key, last)) = file.remove_entry(LAST_REPOSITORY) else {
        return;
    };
    if has_list {
        return;
    }
    // A `recent` that isn't a list counts as none, so `last_repository` takes its place.
    drop(file.remove(RECENT));
    let parsed = last
        .as_str()
        .and_then(|last| RepositoryName::parse(last).ok());
    let mut recent: Array = parsed
        .map(|last| recent_entry(&last, None))
        .into_iter()
        .collect();
    one_per_line(&mut recent);
    let mut recent = Value::Array(recent);
    *recent.decor_mut() = last
        .as_value()
        .map(Value::decor)
        .cloned()
        .unwrap_or_default();
    let key = Key::new(RECENT).with_leaf_decor(key.leaf_decor().clone());
    drop(file.insert_formatted(&key, Item::Value(recent)));
    let place = |key: &Key| order.iter().position(|one| one == key.get());
    file.sort_values_by(|one, _, other, _| place(one).cmp(&place(other)));
}

/// Puts `name` at the head of `file`'s `recent`, opened at `opened_at`, with the comments the user wrote on
/// its entry, and takes that entry out. An entry Tsugiki can't read stays where it is. A `recent` that isn't
/// a list gives the list its place and its comments.
fn put_first_in_recent(file: &mut Table, name: &RepositoryName, opened_at: OffsetDateTime) {
    let mut recent = file
        .get(RECENT)
        .and_then(Item::as_array)
        .cloned()
        .unwrap_or_default();
    let mut head = recent_entry(name, Some(opened_at));
    let held = recent.iter().position(|entry| names(entry, name));
    if let Some(index) = held {
        *head.decor_mut() = recent.remove(index).decor().clone();
    }
    recent.insert_formatted(0, head);
    one_per_line(&mut recent);
    merge_value(file, RECENT, &Item::Value(Value::Array(recent)), None);
}

/// An entry of `recent`: `name`, and when it was opened, to the second, in UTC.
fn recent_entry(name: &RepositoryName, opened_at: Option<OffsetDateTime>) -> Value {
    let mut entry = InlineTable::new();
    drop(entry.insert(NAME, Value::from(name.to_string())));
    if let Some(at) = opened_at.and_then(toml_time) {
        drop(entry.insert(OPENED_AT, Value::from(at)));
    }
    entry.fmt();
    Value::InlineTable(entry)
}

/// `at` as TOML writes a time, in UTC and to the second. Nothing for a year TOML can't write.
fn toml_time(at: OffsetDateTime) -> Option<Datetime> {
    let utc = at.to_offset(UtcOffset::UTC);
    Some(Datetime {
        date: Some(toml_edit::Date {
            year: u16::try_from(utc.year())
                .ok()
                .filter(|year| *year <= 9999)?,
            month: u8::from(utc.month()),
            day: utc.day(),
        }),
        time: Some(toml_edit::Time {
            hour: utc.hour(),
            minute: utc.minute(),
            second: Some(utc.second()),
            nanosecond: None,
        }),
        offset: Some(toml_edit::Offset::Z),
    })
}

/// Whether `entry` of `recent` is the one for `name`.
fn names(entry: &Value, name: &RepositoryName) -> bool {
    entry
        .as_inline_table()
        .and_then(|entry| entry.get(NAME))
        .and_then(Value::as_str)
        .and_then(|written| RepositoryName::parse(written).ok())
        .is_some_and(|written| written.is_same(name))
}

/// Puts each entry of `list` on a line of its own, except where the user wrote a comment.
fn one_per_line(list: &mut Array) {
    for entry in list.iter_mut() {
        let commented = entry
            .decor()
            .prefix()
            .and_then(RawString::as_str)
            .is_some_and(|prefix| prefix.contains('#'));
        if !commented {
            entry.decor_mut().set_prefix("\n    ");
        }
    }
    let commented = list
        .trailing()
        .as_str()
        .is_some_and(|after| after.contains('#'));
    if !commented {
        list.set_trailing("\n");
    }
    list.set_trailing_comma(true);
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
/// Beside the values come the problems with values Tsugiki read as their defaults.
fn read_settings(
    folders: &Folders,
    machine: &Machine,
    current: &Settings,
) -> Result<(Settings, Vec<String>), String> {
    let path = settings_file(folders, machine);
    match read_text(&path)? {
        Some(text) => {
            let settings =
                toml_edit::de::from_str(&text).map_err(|cause| located(&path, &text, &cause))?;
            Ok((settings, unread_problems(&path, &text)))
        }
        None if machine.settings_path.is_none() => Ok((Settings::default(), Vec::new())),
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

/// The folder of the user's theme files, beside the settings file at `settings_file`, so a `settings_path`
/// synced from elsewhere brings its themes along (#519).
fn themes_folder(settings_file: &Path) -> PathBuf {
    settings_file.with_file_name("themes")
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
    let line = cause
        .span()
        .map_or_else(String::new, |span| line_of(text, span.start));
    format!("{}{line}: {}", path.display(), cause.message())
}

/// `, line N`, the line byte `offset` of `text` sits on, counted from 1.
fn line_of(text: &str, offset: usize) -> String {
    let earlier_lines = text.bytes().take(offset).filter(|b| *b == b'\n').count();
    format!(", line {}", earlier_lines.saturating_add(1))
}

#[cfg(test)]
mod tests;
