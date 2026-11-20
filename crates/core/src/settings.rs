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
use crate::filters::{FilterQuery, OnWorkbench};
use crate::folders::Folders;
use crate::keys::{Action, Remaps, Unread};
use crate::latest::{self, LatestReader};
use crate::mirror_source::{MirrorSource, MirrorSources};
use crate::repository::RepositoryName;
use crate::repository_settings::{
    NameCheck, RepositoryCheck, RepositorySettings, SavedBy, SettingsSave,
};
use crate::theme::{
    GivenTheme, ThemeChoice, ThemeKind, ThemeLibrary, ThemeName, ThemePick, Themes,
};

mod saving;

const REPOSITORIES: &str = "repositories";
const FILTERS: &str = "filters";
const RECENT: &str = "recent";
/// The keys of an entry in `recent`.
const NAME: &str = "name";
const OPENED_AT: &str = "opened_at";
const RECENT_COMMANDS: &str = "recent_commands";
/// How many commands `recent_commands` keeps, as VS Code's palette keeps 50 by default.
const COMMANDS_KEPT: usize = 50;
const LAST_REPOSITORY: &str = "last_repository";
const PANES: &str = "panes";
const APPEARANCE: &str = "appearance";
const DIFF: &str = "diff";
const KEYS: &str = "keys";
const MIRRORS: &str = "mirrors";
const LANGUAGE_SERVERS: &str = "language_servers";
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
    /// `[keys]`: each action the user gave another key, or none, in the file's order (#549).
    #[serde(deserialize_with = "remaps")]
    #[specta(type = BTreeMap<Action, String>)]
    pub keys: Remaps,
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

/// What the Settings dialog's Apply changed: each table it edited, and nothing for the others, so a hand
/// edit to anything else made while the dialog was open stays (#520).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Type)]
#[serde(default)]
pub struct SettingsChanges {
    pub status_bar: Option<StatusBarSettings>,
    pub releases: Option<ReleaseSettings>,
    /// The keys of `[appearance]` Settings › General changed (#757).
    pub appearance: Option<AppearanceChanges>,
    /// `[language_servers]` in `machine.toml`, which Settings › Advanced edits (#683).
    pub language_servers: Option<LanguageServerSettings>,
}

impl SettingsChanges {
    /// `settings` with the changes to `settings.toml` made to them, or nothing if none change it. It refuses
    /// a Theme picked by a name `theme` reads otherwise.
    fn made_to(&self, mut settings: Settings) -> Result<Option<Settings>, Error> {
        if self.status_bar.is_none() && self.releases.is_none() && self.appearance.is_none() {
            return Ok(None);
        }
        if let Some(status_bar) = &self.status_bar {
            settings.status_bar = status_bar.clone();
        }
        if let Some(releases) = &self.releases {
            settings.releases = releases.clone();
        }
        if let Some(appearance) = &self.appearance {
            appearance.made_to(&mut settings.appearance)?;
        }
        Ok(Some(settings))
    }
}

/// What the Settings dialog's Apply changed in `[appearance]`: each key it changed, and nothing for the
/// others, so a key it left keeps what the file says then (#757). That keeps a mode from before (#755) and a
/// name no Theme has, which the dialog shows but can't pick.
///
/// A Theme picked by `system`, `light` or `dark` is refused, as it would read back as something else. The
/// dialog lists only the Themes the window holds, so Apply doesn't read the `themes` folder again to check
/// any other name. One whose file has gone since then shows as a hand edit's would: a `theme` follows the
/// system, a side shows its default, and the status bar says so.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Type)]
#[serde(default)]
pub struct AppearanceChanges {
    /// Follow the system, or show the Theme named.
    pub theme: Option<ThemePick>,
    pub light_theme: Option<ThemeName>,
    pub dark_theme: Option<ThemeName>,
}

impl AppearanceChanges {
    fn made_to(&self, appearance: &mut AppearanceSettings) -> Result<(), Error> {
        if let Some(pick) = &self.theme {
            appearance.theme = match pick {
                ThemePick::System => ThemeChoice::System,
                ThemePick::Named { name } => ThemeChoice::named(name.clone())?,
            };
        }
        if let Some(name) = &self.light_theme {
            appearance.light_theme = name.clone();
        }
        if let Some(name) = &self.dark_theme {
            appearance.dark_theme = name.clone();
        }
        Ok(())
    }
}

/// The `[appearance]` table: the theme the window shows, or "system" to follow the system to its light or
/// dark side, and the theme of each side, which following the system shows (#517, #755). A key Tsugiki
/// can't read takes its default, and `unread_problems` says why: a theme is too small a thing to lose the
/// rest of `settings.toml` over. An `appearance` that isn't a table fails the file, as any other table does.
///
/// A theme's name may be a built-in theme's or a file's of the `themes` folder beside the settings file
/// (#519). It stays as the user wrote it: the folder can change while Tsugiki runs, so the `themes` command
/// checks it against the folder each time. A side's name no theme has shows the side's default until one
/// does, and a `theme` no theme has follows the system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct AppearanceSettings {
    pub theme: ThemeChoice,
    pub light_theme: ThemeName,
    pub dark_theme: ThemeName,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::default(),
            light_theme: ThemeName::default_of(ThemeKind::Light),
            dark_theme: ThemeName::default_of(ThemeKind::Dark),
        }
    }
}

/// The key of `[appearance]` that holds the theme chosen for side `kind`.
fn side_key(kind: ThemeKind) -> &'static str {
    match kind {
        ThemeKind::Light => "light_theme",
        ThemeKind::Dark => "dark_theme",
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
    theme: Option<Read<String>>,
    light_theme: Option<Read<ThemeName>>,
    dark_theme: Option<Read<ThemeName>>,
}

impl AppearanceRead {
    /// The values, with each one Tsugiki can't read at its default, and the keys that held those.
    fn settle(self) -> (AppearanceSettings, Vec<Unread>) {
        let mut unread = Vec::new();
        let theme = match self.theme {
            None => ThemeChoice::System,
            Some(Read::Given(text)) => ThemeChoice::read(&text),
            Some(Read::Other(_)) => {
                let problem = r#"isn't "system" or a theme's name, so Tsugiki follows the system."#;
                unread.push(Unread {
                    key: "theme".to_owned(),
                    problem: problem.to_owned(),
                });
                ThemeChoice::System
            }
        };
        let mut side = |given: Option<Read<ThemeName>>, kind: ThemeKind| {
            let default = ThemeName::default_of(kind);
            match given {
                None => default,
                Some(Read::Given(name)) => name,
                Some(Read::Other(_)) => {
                    let instead = default.as_str();
                    let problem = format!("isn't a theme's name, so Tsugiki uses {instead}.");
                    unread.push(Unread {
                        key: side_key(kind).to_owned(),
                        problem,
                    });
                    default
                }
            }
        };
        let light_theme = side(self.light_theme, ThemeKind::Light);
        let dark_theme = side(self.dark_theme, ThemeKind::Dark);
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
            unread.push(Unread {
                key: key.to_owned(),
                problem: instead(),
            });
            T::default()
        }
    }
}

/// The `[diff]` table: how the Files tab shows a PR's diff (#605) and where the Definition peek opens (#459).
/// A value Tsugiki can't read takes its default, and `unread_problems` says why, as for `[appearance]`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Type)]
pub struct DiffSettings {
    pub files: DiffFiles,
    pub peek: DiffPeek,
}

/// Whether a PR opens on all its files, one under the other, or on one file at a time: `[diff] files`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DiffFiles {
    #[default]
    All,
    One,
}

/// The Definition peek's place, `[diff] peek` (#459): under its line, among the rows of the code, or split
/// below the code, which stays where it was.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DiffPeek {
    #[default]
    Inline,
    Split,
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
    peek: Option<Read<DiffPeek>>,
}

impl DiffRead {
    /// The values, with each one Tsugiki can't read at its default, and the keys that held those.
    fn settle(self) -> (DiffSettings, Vec<Unread>) {
        let mut unread = Vec::new();
        let settings = DiffSettings {
            files: given_or_default(self.files, "files", &mut unread, || {
                r#"isn't "all" or "one", so each PR opens on all its files."#.to_owned()
            }),
            peek: given_or_default(self.peek, "peek", &mut unread, || {
                r#"isn't "inline" or "split", so the Definition peek opens under its line."#
                    .to_owned()
            }),
        };
        (settings, unread)
    }
}

/// `[keys]` as the file gives it: each line's action name and value, in the file's order, with no value for
/// one that isn't text.
struct KeysRead(Vec<(String, Option<String>)>);

impl<'de> Deserialize<'de> for KeysRead {
    fn deserialize<D: Deserializer<'de>>(given: D) -> Result<Self, D::Error> {
        /// Reads the lines one by one, as a map would lose their order.
        struct Lines;

        impl<'de> serde::de::Visitor<'de> for Lines {
            type Value = KeysRead;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a table of actions and their keys")
            }

            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<KeysRead, M::Error> {
                let mut lines = Vec::new();
                while let Some((name, value)) = map.next_entry::<String, Read<String>>()? {
                    lines.push((name, value.given()));
                }
                Ok(KeysRead(lines))
            }
        }

        given.deserialize_map(Lines)
    }
}

/// `[keys]` as `Settings` holds it: the lines Tsugiki can read, and the problems with the others, which
/// `SettingsText::key_problems` puts in the status bar's words. A line Tsugiki can't read leaves its action
/// at its default key, as a value of `[appearance]` does.
fn remaps<'de, D: Deserializer<'de>>(given: D) -> Result<Remaps, D::Error> {
    Ok(Remaps::read(KeysRead::deserialize(given)?.0))
}

/// The problems with the `[appearance]` and `[diff]` of the settings file `text`, from `path`, in words for
/// the status bar. Each names its line, when the parser knows it.
fn unread_problems(path: &Path, text: &str) -> Vec<String> {
    SettingsText::parse(path, text).unread_problems()
}

/// A settings file's text, parsed once for the places of its keys.
struct SettingsText<'a> {
    path: &'a Path,
    text: &'a str,
    document: Option<toml_edit::Document<&'a str>>,
}

impl<'a> SettingsText<'a> {
    fn parse(path: &'a Path, text: &'a str) -> Self {
        Self {
            path,
            text,
            document: toml_edit::Document::parse(text).ok(),
        }
    }

    /// The problems with the `[appearance]` and `[diff]` of the file, as `unread_problems` gives them.
    fn unread_problems(&self) -> Vec<String> {
        /// Only the tables of the file whose values fall back to their defaults.
        #[derive(Default, Deserialize)]
        #[serde(default)]
        struct File {
            appearance: AppearanceRead,
            diff: DiffRead,
        }
        // Each caller has read the whole file by now, so this read finds what that one did.
        let file = toml_edit::de::from_str::<File>(self.text).unwrap_or_default();
        [
            (APPEARANCE, file.appearance.settle().1),
            (DIFF, file.diff.settle().1),
        ]
        .into_iter()
        .flat_map(|(table, unread)| self.described(table, &unread))
        .collect()
    }

    /// The problems with the lines of the file's `[keys]`, read as `keys`, in words for the status bar.
    /// Unlike those of `unread_problems`, they don't refuse a save, which leaves `[keys]` as the user wrote
    /// it.
    fn key_problems(&self, keys: &Remaps) -> Vec<String> {
        self.described(KEYS, keys.unread())
    }

    /// The problems `unread` with keys of the table `table`, each after its place.
    fn described(&self, table: &str, unread: &[Unread]) -> Vec<String> {
        unread
            .iter()
            .map(|Unread { key, problem }| format!("{}: {key} {problem}", self.place(table, key)))
            .collect()
    }

    /// The file and, when the parser knows it, the line of `key` in the table `table`.
    fn place(&self, table: &str, key: &str) -> String {
        let line = self
            .document
            .as_ref()
            .and_then(|document| document.get(table))
            .and_then(Item::as_table_like)
            .and_then(|table| table.get_key_value(key))
            .and_then(|(key, _)| key.span())
            .map_or_else(String::new, |span| line_of(self.text, span.start));
        format!("{}{line}", self.path.display())
    }
}

/// The settings in force, and what is wrong with the files they came from, in words for the status bar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Type)]
pub struct LoadedSettings {
    pub settings: Settings,
    /// `[language_servers]` from `machine.toml`, which Settings › Advanced shows (#683).
    pub language_servers: LanguageServerSettings,
    pub problems: Vec<String>,
}

/// `[language_servers]` in `machine.toml` as Settings › Advanced shows and changes it (#683): each server's
/// path as the file gives it, which may be relative to `machine.toml`'s folder, and the idle time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct LanguageServerSettings {
    pub ty: Option<PathBuf>,
    #[serde(rename = "rust-analyzer")]
    pub rust_analyzer: Option<PathBuf>,
    pub clangd: Option<PathBuf>,
    /// The minutes a server that no lookup reaches runs before it stops.
    pub idle_minutes: NonZeroU32,
}

impl Default for LanguageServerSettings {
    fn default() -> Self {
        Self {
            ty: None,
            rust_analyzer: None,
            clangd: None,
            idle_minutes: IDLE_MINUTES,
        }
    }
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
    /// The place of `[appearance] theme`: the settings file and, when it has the key, its line, for the
    /// status bar to name when no theme has the name it gives (#755).
    pub theme_place: String,
    pub language_servers: LanguageServers,
    /// How many saves of a Repository's settings the owner has handed the session, those this view shows
    /// among them. The owner publishes a view before it hands its saves over, so a session that has taken
    /// fewer knows the rest are on their way.
    pub saves_told: u64,
}

impl SettingsView {
    /// The settings of the Repository `name`, or the defaults for one the settings don't list.
    pub fn repository(&self, name: RepositoryName) -> RepositorySettings {
        self.loaded.settings.repository(name)
    }

    /// The themes for the window, from the built-in themes and the `themes` folder read now, showing what
    /// `appearance` asks for: the file's `[appearance]`, or what Settings › General shows before Apply saves
    /// it (#757). Both go by the same rules, so the dialog's preview is what Apply shows.
    pub fn themes(&self, appearance: &AppearanceSettings) -> Result<Themes, Error> {
        let library = ThemeLibrary::read(&self.themes_folder)?;
        let given = GivenTheme {
            choice: &appearance.theme,
            place: &self.theme_place,
        };
        Ok(library.themes(given, &appearance.light_theme, &appearance.dark_theme))
    }
}

/// A command of the command palette, by the id the window gives it. The window names its commands, so the
/// core keeps the id as it comes and checks nothing in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct CommandId(String);

impl CommandId {
    pub fn new(id: &str) -> Self {
        Self(id.to_owned())
    }

    fn as_str(&self) -> &str {
        &self.0
    }
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

impl TokenSource {
    /// Where this run's token comes from: gh, the only source until Tsugiki reads a personal access token
    /// from the keychain (#430).
    pub const IN_USE: Self = Self::Gh;
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
            token: TokenSource::IN_USE,
        }
    }
}

/// How the user laid out the panes: how wide they dragged the Sidebar and the Workbench list, in CSS pixels
/// (#187), whether the Sidebar (#234) and the list (#242) show, whether the Sidebar shows as icons (#587),
/// which of them `Ctrl+K Z` hid (#246), to bring back on the next press, the list's order (#592), how many
/// rows tall they dragged the Definition peek (#458), how many pixels tall they dragged the split one
/// (#459), and how wide they dragged the PR view's rail (#603). All of it stays on this machine, like the
/// window's place, and a value left out takes its default. So does a value Tsugiki can't read, such as a
/// width that isn't a whole number of pixels: a pane is too small a thing to lose the rest of
/// `machine.toml` over.
#[expect(
    clippy::struct_excessive_bools,
    reason = "Each pane hides by itself, and a hidden Sidebar keeps whether it comes back as icons, so \
              every mix is valid: both hidden leaves the Detail pane"
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
    /// The split Definition peek's height in CSS pixels, or nothing for 42% of the room it shares with the
    /// code above it, the diff's or the file view's (#459).
    #[serde(deserialize_with = "or_default")]
    #[specta(type = Option<u32>)]
    pub split_peek: Option<u32>,
    /// The width of the rail beside the PR's Conversation and Files tabs in CSS pixels, or nothing for its
    /// 280 (#603).
    #[serde(deserialize_with = "or_default")]
    #[specta(type = Option<u32>)]
    pub rail: Option<u32>,
    /// `Ctrl+B` hid the Sidebar. It keeps its width for when it shows again.
    #[serde(deserialize_with = "or_default")]
    #[specta(type = bool)]
    pub sidebar_hidden: bool,
    /// The Sidebar shows as a strip of icons, or will once `Ctrl+B` brings it back (#587). It keeps its width
    /// for when it shows in full again.
    #[serde(deserialize_with = "or_default")]
    #[specta(type = bool)]
    pub sidebar_icons: bool,
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

/// How the Issue list orders its items (#592).
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

/// The commands in `recent_commands`, leaving out an entry that isn't text, for the reason `sources_or_skip`
/// gives, as `KeptCommands` keeps them. A `recent_commands` that isn't a list counts as none.
fn commands_or_skip<'de, D: Deserializer<'de>>(given: D) -> Result<Vec<CommandId>, D::Error> {
    let entries = Read::<Vec<Read<CommandId>>>::deserialize(given)?.given();
    let mut kept = KeptCommands::default();
    for command in entries.into_iter().flatten().filter_map(Read::given) {
        if !kept.keep(command) && kept.full() {
            break;
        }
    }
    Ok(kept.0)
}

/// The commands `recent_commands` keeps, in its order: each once, where it first comes, and only the first
/// `COMMANDS_KEPT`. A hand edit may have written one twice, or more than that. Reading the file and writing
/// it both go through `keep`, so what a run saves is what the next read finds.
#[derive(Default)]
struct KeptCommands(Vec<CommandId>);

impl KeptCommands {
    /// Whether the list keeps `command`, which it then holds.
    fn keep(&mut self, command: CommandId) -> bool {
        let keeps = !self.full() && !self.0.contains(&command);
        if keeps {
            self.0.push(command);
        }
        keeps
    }

    fn full(&self) -> bool {
        self.0.len() == COMMANDS_KEPT
    }
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
    #[serde(deserialize_with = "commands_or_skip")]
    recent_commands: Vec<CommandId>,
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

impl ServersFile {
    /// What the file says, as Settings › Advanced shows it.
    fn settings(&self) -> LanguageServerSettings {
        let path = |server: &Option<ServerFile>| server.as_ref().map(|file| file.path.clone());
        LanguageServerSettings {
            ty: path(&self.ty),
            rust_analyzer: path(&self.rust_analyzer),
            clangd: path(&self.clangd),
            idle_minutes: self.idle_minutes.unwrap_or(IDLE_MINUTES),
        }
    }
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
    /// The command palette's commands run on this machine, most recently run first.
    recent_commands: Vec<CommandId>,
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
            recent_commands: file.recent_commands,
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
    /// The place of `[appearance] theme` in the settings file last read.
    theme_place: String,
}

impl SettingsFiles {
    /// Reads both files. Anything wrong with them leaves the defaults in force.
    pub fn load(folders: Folders) -> Self {
        let mut files = Self {
            folders,
            machine: None,
            loaded: LoadedSettings::default(),
            theme_place: String::new(),
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
            theme_place: self.theme_place.clone(),
            language_servers: {
                let given = &self.loaded.language_servers;
                let path =
                    |path: &Option<PathBuf>| path.as_ref().map(|path| self.folders.data.join(path));
                LanguageServers {
                    ty: path(&given.ty),
                    rust_analyzer: path(&given.rust_analyzer),
                    clangd: path(&given.clangd),
                    idle: minutes(given.idle_minutes),
                }
            },
            saves_told: 0,
        }
    }

    /// The command palette's commands the user ran on this machine, most recently run first (#556). Apart
    /// from `view`, as only the palette asks for them.
    pub fn recent_commands(&self) -> Vec<CommandId> {
        self.machine
            .as_ref()
            .map_or_else(Vec::new, |machine| machine.recent_commands.clone())
    }

    /// Reads both files again, for the Settings window to call as it opens.
    pub fn reload(&mut self) {
        let mut problems = Vec::new();
        match read_machine(&self.folders.machine_file()) {
            Ok(machine) => self.machine = Some(machine),
            Err(problem) => problems.push(problem),
        }
        if let Some(machine) = &self.machine {
            self.loaded.language_servers = machine.servers.settings();
            match read_settings(&self.folders, machine, &self.loaded.settings) {
                Ok((settings, unread, theme_place)) => {
                    self.loaded.settings = settings;
                    self.theme_place = theme_place;
                    problems.extend(unread);
                }
                Err(problem) => {
                    // The settings kept came from text the file no longer holds, so no line of it counts.
                    let path = settings_file(&self.folders, machine);
                    self.theme_place = path.display().to_string();
                    problems.push(problem);
                }
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
            without_hand_edits(as_document(settings)).as_table(),
            without_hand_edits(as_document(&Settings::default())).as_table(),
        );
        edit.save()?;
        self.reload();
        Ok(())
    }

    /// Saves what the Settings dialog's Apply changed over what the files say now, and reads the files
    /// again. A hand edit made while the dialog was open stays, unless the dialog changed the same table.
    /// Each file is saved only if the changes touch it. `machine.toml` is read and edited before
    /// `settings.toml` is written, so one that doesn't parse refuses the whole Apply with nothing saved,
    /// and the dialog's Cancel still matches the files.
    pub fn apply(&mut self, changes: SettingsChanges) -> Result<(), Error> {
        self.reload();
        let machine = self.folders.machine_file();
        let servers = (changes.language_servers.as_ref())
            .map(|servers| Self::language_servers_edit(&machine, servers))
            .transpose()?;
        if let Some(settings) = changes.made_to(self.loaded.settings.clone())? {
            self.write(&settings)?;
        }
        if let Some(edit) = servers {
            edit.save()?;
            self.reload();
        }
        Ok(())
    }

    /// `machine.toml` at `path` with `[language_servers]` as Settings › Advanced left it, ready to save,
    /// keeping everything else in the file. A server without a path loses its `path`, and an idle time back
    /// at its default goes, unless the user commented on it. It refuses while `machine.toml` can't be used.
    fn language_servers_edit<'a>(
        path: &'a Path,
        servers: &LanguageServerSettings,
    ) -> Result<TomlEdit<'a, Machine>, Error> {
        let mut edit = TomlEdit::open_machine(path)?;
        edit_section(edit.document.as_table_mut(), LANGUAGE_SERVERS, |section| {
            let minutes = |count: NonZeroU32| toml_edit::value(i64::from(count.get()));
            let default = minutes(IDLE_MINUTES);
            merge_value(
                section,
                "idle_minutes",
                &minutes(servers.idle_minutes),
                Some(&default),
            );
            for (key, path) in [
                ("ty", &servers.ty),
                ("rust-analyzer", &servers.rust_analyzer),
                ("clangd", &servers.clangd),
            ] {
                edit_section(section, key, |server| match path {
                    // The same path stays as the user quoted it.
                    Some(path) if server.get("path").and_then(Item::as_str) == path.to_str() => {}
                    Some(path) => {
                        let path = toml_edit::value(path.to_string_lossy().into_owned());
                        merge_value(server, "path", &path, None);
                    }
                    None => drop(server.remove("path")),
                });
            }
        });
        Ok(edit)
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
        let order = first_of_each(reopened.chain(repositories));
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
                ("split_peek", panes.split_peek),
                ("rail", panes.rail),
            ] {
                // A reset takes the comment on its key too: kept, the key would hold the old size.
                match size {
                    Some(size) => {
                        merge_value(section, key, &toml_edit::value(i64::from(size)), None)
                    }
                    None => drop(section.remove(key)),
                }
            }
            for (key, set) in [
                ("sidebar_hidden", panes.sidebar_hidden),
                ("sidebar_icons", panes.sidebar_icons),
                ("list_hidden", panes.list_hidden),
            ] {
                merge_value(
                    section,
                    key,
                    &toml_edit::value(set),
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

    /// Puts `command` at the head of `recent_commands` in `machine.toml`, keeping everything else in it. It
    /// refuses while `machine.toml` can't be used.
    ///
    /// It answers with the list it saved, and takes in that list and nothing else: a palette run is no
    /// moment to read the files again (ADR 0014), and `settings.toml` may sit on a slow drive. Until a good
    /// `machine.toml` has been read, only the answer holds the list, and `recent_commands` waits for the next
    /// read.
    pub fn remember_command(&mut self, command: &CommandId) -> Result<Vec<CommandId>, Error> {
        let path = self.folders.machine_file();
        let mut edit = TomlEdit::open_machine(&path)?;
        let saved = put_first_in_recent_commands(edit.document.as_table_mut(), command);
        edit.save()?;
        if let Some(machine) = &mut self.machine {
            machine.recent_commands.clone_from(&saved);
        }
        Ok(saved)
    }

    /// Saves the theme picker's choice to `[appearance] theme`, keeping everything else in `settings.toml`,
    /// and reads the files again (#518, #756). Like `write`, it works from what the files say now and refuses
    /// while either can't be used. `light_theme` and `dark_theme` stay: only Settings and a hand edit change
    /// them. A mode from before (#755) gives way to the choice. Following the system is the default, so it
    /// leaves the file, unless the user wrote a comment on the key.
    pub fn save_theme(&mut self, choice: &ThemeChoice) -> Result<(), Error> {
        let machine = self.machine_to_edit()?;
        let path = settings_file(&self.folders, &machine);
        let mut edit = TomlEdit::<Settings>::open(&path)?;
        edit_section(edit.document.as_table_mut(), APPEARANCE, |section| {
            merge_value(
                section,
                "theme",
                &toml_edit::value(String::from(choice.clone())),
                Some(&toml_edit::value(ThemeChoice::SYSTEM)),
            );
        });
        edit.save()?;
        self.reload();
        Ok(())
    }

    /// Puts `name`'s Filter that searches `query` on the Workbench, or takes it off, with its `on_workbench`
    /// in `settings.toml`, keeping everything else in the file, and reads the files again (#819). Of two
    /// Filters with that search it switches the first, the one the settings read. A Repository the file holds
    /// without `filters` gets the Filters it reads as written out first. Like `write`, it works from what the
    /// files say now and refuses while either can't be used. It refuses a Repository or a search the file
    /// doesn't hold.
    pub fn put_on_workbench(
        &mut self,
        name: &RepositoryName,
        query: &FilterQuery,
        on: OnWorkbench,
    ) -> Result<(), Error> {
        let machine = self.machine_to_edit()?;
        let path = settings_file(&self.folders, &machine);
        let mut edit = TomlEdit::<Settings>::open(&path)?;
        let held = (edit.values.repositories.iter())
            .find(|one| one.name.is_same(name))
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::NotFound,
                    &format!("Tsugiki doesn't have {name}."),
                )
            })?;
        let written = as_document(held).remove(FILTERS);
        let switched = (edit.document.get_mut(REPOSITORIES))
            .and_then(|repositories| filters_in_file(repositories, name, written))
            .is_some_and(|filters| switch_filter(filters, query, on));
        if !switched {
            let message = format!("{name} has no Filter that searches {}.", query.as_str());
            return Err(Error::new(ErrorKind::NotFound, &message));
        }
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
#[derive(Clone)]
pub struct SettingsOwner {
    requests: Sender<Request>,
    view: LatestReader<SettingsView>,
    /// The palette's recent commands, apart from `view`, which every command that reads the settings copies.
    commands: LatestReader<Vec<CommandId>>,
}

/// The settings owner's thread on its way to its end, from [`SettingsOwner::begin_stopping`].
#[must_use = "only `wait_for_end` waits for the save under way"]
pub struct SettingsEnding(Receiver<()>);

impl SettingsEnding {
    /// Waits up to `patience` for the thread to end. A save that outlasts it, on a drive gone slow, goes on
    /// until the process ends, and then the file holds its old text (`saving::save`).
    pub fn wait_for_end(self, patience: Duration) {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "running out of patience is the one failure here, and the doc above says why it is safe"
        )]
        let _ = self.0.recv_timeout(patience);
    }
}

/// Where the owner's thread hands each Repository's settings a change saves otherwise, with those they
/// replaced: the session's queue, which `fetching`, above this module, owns.
pub type SettingsSink = Box<dyn FnMut(SettingsSave) + Send>;

/// What the owner's thread is asked.
enum Request {
    Change(Change),
    /// Hand each Repository's settings that a change saves otherwise to the session from now on.
    Session(SettingsSink),
    /// The palette ran `command`. The answer is the list saved, as no one needs the view after a run.
    Command {
        command: CommandId,
        reply: Sender<Result<Vec<CommandId>, Error>>,
    },
    /// End, having answered what was asked before, as `begin_stopping` asks. `SettingsEnding::wait_for_end`
    /// waits for `finishing` to go, which it does as the thread ends, and also if the thread has gone before
    /// it takes this.
    Stop {
        finishing: Sender<()>,
    },
}

impl From<Change> for Request {
    fn from(change: Change) -> Self {
        Self::Change(change)
    }
}

/// What the owner's thread saves, and where it answers with what the settings say after.
enum Change {
    Add {
        name: RepositoryName,
        source: MirrorSource,
        reply: Sender<Result<SettingsView, Error>>,
    },
    OnWorkbench {
        name: RepositoryName,
        query: FilterQuery,
        on: OnWorkbench,
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
        choice: ThemeChoice,
        reply: Sender<Result<SettingsView, Error>>,
    },
    Open {
        name: RepositoryName,
        reply: Sender<Result<SettingsView, Error>>,
    },
    Reread {
        reply: Sender<Result<SettingsView, Error>>,
    },
    Apply {
        changes: SettingsChanges,
        reply: Sender<Result<SettingsView, Error>>,
    },
}

impl SettingsOwner {
    /// From here on only this thread can read `files`, and `view` answers from its copy.
    pub fn start(mut files: Result<SettingsFiles, Error>) -> Self {
        let first = files.as_ref().map(SettingsFiles::view).unwrap_or_default();
        let (mut writer, view) = latest::channel(first);
        let mut published = files
            .as_ref()
            .map(SettingsFiles::recent_commands)
            .unwrap_or_default();
        let (mut commands_writer, commands) = latest::channel(published.clone());
        let (requests, incoming) = mpsc::channel::<Request>();
        #[expect(
            unused_results,
            reason = "it ends when `begin_stopping` asks or once the last sender goes, and `wait_for_end` \
                      waits for it with a patience a join can't take"
        )]
        thread::spawn(move || {
            // Sends readers the palette's recent commands, unless they hold this list already.
            let mut publish_if_changed = |commands: &Vec<CommandId>| {
                if *commands != published {
                    commands_writer.set(commands);
                    published.clone_from(commands);
                }
            };
            let mut session: Option<ToldSession> = None;
            for request in incoming {
                let change = match request {
                    Request::Change(change) => change,
                    Request::Session(sink) => {
                        let told = (files.as_ref().ok())
                            .map(|files| files.view().loaded.settings.repositories)
                            .unwrap_or_default();
                        session = Some(ToldSession {
                            sink,
                            told,
                            saves: 0,
                        });
                        continue;
                    }
                    Request::Command { command, reply } => {
                        let saved = match &mut files {
                            Ok(files) => files.remember_command(&command),
                            Err(problem) => Err(problem.clone()),
                        };
                        if let Ok(saved) = &saved {
                            publish_if_changed(saved);
                        }
                        answer(&reply, saved);
                        continue;
                    }
                    Request::Stop {
                        finishing: _finishing,
                    } => break,
                };
                // Only the Sidebar's switch saves a Repository's settings from the window. Every other change
                // reloads the files, and so takes in a hand edit to them.
                let by = match change {
                    Change::OnWorkbench { .. } => SavedBy::Window,
                    Change::Add { .. }
                    | Change::MirrorSource { .. }
                    | Change::Panes { .. }
                    | Change::Theme { .. }
                    | Change::Open { .. }
                    | Change::Reread { .. }
                    | Change::Apply { .. } => SavedBy::HandEdit,
                };
                let (mut saved, reply) = match (&mut files, change) {
                    (
                        Ok(files),
                        Change::Add {
                            name,
                            source,
                            reply,
                        },
                    ) => {
                        let saved = files.add_repository(
                            &RepositorySettings::new(name),
                            &source,
                            OffsetDateTime::now_utc(),
                        );
                        (saved.map(|_| files.view()), reply)
                    }
                    (
                        Ok(files),
                        Change::OnWorkbench {
                            name,
                            query,
                            on,
                            reply,
                        },
                    ) => {
                        let saved = files.put_on_workbench(&name, &query, on);
                        (saved.map(|()| files.view()), reply)
                    }
                    (
                        Ok(files),
                        Change::MirrorSource {
                            name,
                            source,
                            reply,
                        },
                    ) => {
                        let saved = files.save_mirror_source(&name, &source);
                        (saved.map(|()| files.view()), reply)
                    }
                    (Ok(files), Change::Panes { layout, reply }) => {
                        (files.save_panes(layout).map(|()| files.view()), reply)
                    }
                    (Ok(files), Change::Theme { choice, reply }) => {
                        (files.save_theme(&choice).map(|()| files.view()), reply)
                    }
                    (Ok(files), Change::Open { name, reply }) => {
                        let opened = files.open_repository(&name, OffsetDateTime::now_utc());
                        (opened.map(|_| files.view()), reply)
                    }
                    (Ok(files), Change::Reread { reply }) => {
                        files.reload();
                        (Ok(files.view()), reply)
                    }
                    (Ok(files), Change::Apply { changes, reply }) => {
                        (files.apply(changes).map(|()| files.view()), reply)
                    }
                    (
                        Err(problem),
                        Change::Add { reply, .. }
                        | Change::OnWorkbench { reply, .. }
                        | Change::MirrorSource { reply, .. }
                        | Change::Panes { reply, .. }
                        | Change::Theme { reply, .. }
                        | Change::Open { reply, .. }
                        | Change::Reread { reply }
                        | Change::Apply { reply, .. },
                    ) => (Err(problem.clone()), reply),
                };
                // Before the answer, so whoever asked finds the change in `view`. Changes come through here
                // one at a time, so an older one can't replace a newer one.
                if let (Ok(saved), Ok(files)) = (&mut saved, &files) {
                    let saves = (session.as_mut()).map(|told| told.saves(saved, by));
                    writer.set(saved);
                    // Each change reads `machine.toml` again, which may hold a hand edit to the list.
                    publish_if_changed(&files.recent_commands());
                    // Before the answer too, so the session hears of the change before whatever the one who
                    // asked sends it next, and from this thread alone, in the order the changes saved.
                    if let (Some(told), Some(saves)) = (&mut session, saves) {
                        told.hand_over(saves);
                    }
                }
                answer(&reply, saved);
            }
        });
        Self {
            requests,
            view,
            commands,
        }
    }

    /// What the settings said after the last change saved. This never waits on a file.
    pub fn view(&self) -> SettingsView {
        self.view.get()
    }

    /// The command palette's commands the user ran on this machine, most recently run first, as the owner
    /// last read or saved them. This never waits on a file.
    pub fn recent_commands(&self) -> Vec<CommandId> {
        self.commands.get()
    }

    /// From now on, hands `session` each Repository's settings that a change saves otherwise, with those
    /// they replaced and what saved them: from the owner's thread, before it answers, in the order the
    /// changes saved. So no copy a command read earlier can reach the session after a newer one, and a hand
    /// edit reaches it whichever change reloaded the files first (#822).
    pub fn tell_session(&self, session: SettingsSink) {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "the owner ends only once the last sender goes, so this fails only if it panicked, and \
                      then nothing saves"
        )]
        let _ = self.requests.send(Request::Session(session));
    }

    /// Tells the thread to end once it has answered what it was asked before, the save under way among them.
    /// Every copy of this owner then refuses each request with `STOPPED_SAVING`. The caller goes on with its
    /// own stopping, and the save with it, until it waits on what this returns.
    pub fn begin_stopping(&self) -> SettingsEnding {
        let (finishing, finished) = mpsc::channel::<()>();
        // A thread already gone hands the request back in the error, which `finishing` goes with.
        drop(self.requests.send(Request::Stop { finishing }));
        SettingsEnding(finished)
    }

    /// A copy of what the settings say, for a thread that reads them later.
    pub fn reader(&self) -> LatestReader<SettingsView> {
        self.view.clone()
    }

    /// Adds `name` to the settings, and answers with what they say now.
    pub fn add_repository(
        &self,
        name: RepositoryName,
        source: MirrorSource,
    ) -> Result<SettingsView, Error> {
        let (reply, answer) = mpsc::channel();
        self.ask(
            Change::Add {
                name,
                source,
                reply,
            },
            &answer,
        )
    }

    /// Puts `name`'s Filter that searches `query` on the Workbench, or takes it off, in `settings.toml`
    /// (`SettingsFiles::put_on_workbench`), and answers with what the settings say now.
    pub fn put_on_workbench(
        &self,
        name: RepositoryName,
        query: FilterQuery,
        on: OnWorkbench,
    ) -> Result<SettingsView, Error> {
        let (reply, answer) = mpsc::channel();
        self.ask(
            Change::OnWorkbench {
                name,
                query,
                on,
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
            Change::MirrorSource {
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
        self.ask(Change::Panes { layout, reply }, &answer)
    }

    /// Puts `command` at the head of the palette's recent commands in `machine.toml`, and answers with the
    /// list saved, most recently run first.
    pub fn remember_command(&self, command: CommandId) -> Result<Vec<CommandId>, Error> {
        let (reply, answer) = mpsc::channel();
        self.ask(Request::Command { command, reply }, &answer)
    }

    /// Saves the theme picker's choice to `[appearance] theme` in `settings.toml`, and answers with what the
    /// settings say now.
    pub fn save_theme(&self, choice: ThemeChoice) -> Result<SettingsView, Error> {
        let (reply, answer) = mpsc::channel();
        self.ask(Change::Theme { choice, reply }, &answer)
    }

    /// Reads both files again, as the Settings dialog opens, so a hand edit made since shows in it and takes
    /// effect, and answers with what they say now (ADR 0014).
    pub fn reread(&self) -> Result<SettingsView, Error> {
        let (reply, answer) = mpsc::channel();
        self.ask(Change::Reread { reply }, &answer)
    }

    /// Saves what the Settings dialog's Apply changed (`SettingsFiles::apply`), and answers with what the
    /// settings say now.
    pub fn apply(&self, changes: SettingsChanges) -> Result<SettingsView, Error> {
        let (reply, answer) = mpsc::channel();
        self.ask(Change::Apply { changes, reply }, &answer)
    }

    /// Makes `name` the open Repository, and answers with what the settings say now.
    pub fn open_repository(&self, name: RepositoryName) -> Result<SettingsView, Error> {
        let (reply, answer) = mpsc::channel();
        self.ask(Change::Open { name, reply }, &answer)
    }

    /// What Add repository says about `name` (`Settings::check_name`), from what the files say now: a
    /// Repository the user took out of `settings.toml` by hand is no longer held. Without files to read it
    /// goes by the view. GitHub is asked on this thread, so the owner never waits for it.
    pub fn check_name(
        &self,
        name: RepositoryName,
        ask_github: &mut dyn FnMut(RepositoryName) -> Result<RepositoryCheck, Error>,
    ) -> Result<NameCheck, Error> {
        let view = self.reread().unwrap_or_else(|_| self.view());
        view.loaded.settings.check_name(name, ask_github)
    }

    fn ask<T>(
        &self,
        request: impl Into<Request>,
        answer: &Receiver<Result<T, Error>>,
    ) -> Result<T, Error> {
        self.requests
            .send(request.into())
            .ok()
            .and_then(|()| answer.recv().ok())
            .unwrap_or_else(|| Err(Error::new(ErrorKind::Internal, STOPPED_SAVING)))
    }
}

/// The session the owner hands each save to, with each Repository's settings as it was last told them, and
/// how many saves it has been handed. Of a Repository written twice, the first counts, as in `home`.
///
/// `told` starts as what the files said when the session asked to hear, at launch, which the session never
/// heard. So the first save of a Repository's settings comes with those it replaced too. Files that
/// couldn't be read then list no Repository, and each comes as added once they can. A Repository the files
/// stop listing, as a hand edit that takes it out or a reload that finds no `settings.toml` does, goes to
/// the defaults, which the session reads for it then, so one that comes back moves from those.
struct ToldSession {
    sink: SettingsSink,
    told: Vec<RepositorySettings>,
    saves: u64,
}

impl ToldSession {
    /// The saves a change makes that leaves `view` saying what it says, as `by` made it: each Repository's
    /// settings that differ from those told, and the defaults of each one the files stopped listing. Counts
    /// them in `view`, which the owner publishes before it hands them over.
    fn saves(&mut self, view: &mut SettingsView, by: SavedBy) -> Vec<SettingsSave> {
        let settings = &view.loaded.settings;
        let listed = first_of_each(&settings.repositories);
        let dropped = (self.told.iter())
            .filter(|held| !listed.iter().any(|one| one.name.is_same(&held.name)))
            .map(|held| settings.repository(held.name.clone()));
        let now: Vec<RepositorySettings> =
            (listed.iter().copied().cloned()).chain(dropped).collect();
        let mut saves = Vec::new();
        for repository in now {
            let save = SettingsSave::new(repository.clone(), &self.told, by);
            if save.replaced() == Some(&repository) {
                continue;
            }
            saves.push(save);
            self.saves += 1;
            match (self.told.iter_mut()).find(|held| held.name.is_same(&repository.name)) {
                Some(held) => *held = repository,
                None => self.told.push(repository),
            }
        }
        view.saves_told = self.saves;
        saves
    }

    /// Hands the session `saves`, in order.
    fn hand_over(&mut self, saves: Vec<SettingsSave>) {
        for save in saves {
            (self.sink)(save);
        }
    }
}

/// Each of `repositories` whose name none before it holds, in any case: of a Repository the user wrote
/// twice, the first.
fn first_of_each<'a>(
    repositories: impl IntoIterator<Item = &'a RepositorySettings>,
) -> Vec<&'a RepositorySettings> {
    let mut first: Vec<&RepositorySettings> = Vec::new();
    for repository in repositories {
        if !first.iter().any(|one| one.name.is_same(&repository.name)) {
            first.push(repository);
        }
    }
    first
}

/// Hands `saved` to whoever asked the owner's thread.
fn answer<T>(reply: &Sender<T>, saved: T) {
    #[expect(
        clippy::let_underscore_must_use,
        reason = "this fails only if the caller has gone, and it waits for this answer"
    )]
    let _ = reply.send(saved);
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
        saving::save(self.path, &merged).map_err(|cause| {
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

/// The `filters` of the first table of `repositories` that names `name`. A table without them gets `written`
/// first, the Filters the settings read it as, in blocks of their own like those `append` writes.
fn filters_in_file<'a>(
    repositories: &'a mut Item,
    name: &RepositoryName,
    written: Option<Item>,
) -> Option<&'a mut Item> {
    let names_it = |table: &&mut dyn TableLike| {
        (table.get(NAME).and_then(Item::as_str))
            .and_then(|text| RepositoryName::parse(text).ok())
            .is_some_and(|text| text.is_same(name))
    };
    let repository = tables_of(repositories).into_iter().find(names_it)?;
    if let (false, Some(written)) = (repository.contains_key(FILTERS), written) {
        let blocks = written
            .into_array_of_tables()
            .map_or_else(|kept| kept, Item::ArrayOfTables);
        drop(repository.insert(FILTERS, blocks));
    }
    repository.get_mut(FILTERS)
}

/// Sets `on_workbench` on the first Filter of `filters` that searches `query`, the one the settings read,
/// keeping a comment the user wrote on it, and says whether there was one. A Filter switched off says so,
/// so a hand edit finds the key.
fn switch_filter(filters: &mut Item, query: &FilterQuery, on: OnWorkbench) -> bool {
    let searching = tables_of(filters).into_iter().find(|filter| {
        (filter.get("query").and_then(Item::as_str))
            .is_some_and(|text| text.trim() == query.as_str())
    });
    let Some(filter) = searching else {
        return false;
    };
    merge_value(
        filter,
        "on_workbench",
        &toml_edit::value(bool::from(on)),
        None,
    );
    // A key added to an inline table would follow the space before its `}`: `"label:bug" , on_workbench`.
    let inline = (filters.as_array_mut().into_iter()).flat_map(Array::iter_mut);
    (inline.filter_map(Value::as_inline_table_mut)).for_each(InlineTable::fmt);
    true
}

/// Each table of `list`, written as blocks or inline.
fn tables_of(list: &mut Item) -> Vec<&mut dyn TableLike> {
    if list.is_array_of_tables() {
        let blocks = (list.as_array_of_tables_mut().into_iter()).flat_map(ArrayOfTables::iter_mut);
        return blocks
            .map(|table| -> &mut dyn TableLike { table })
            .collect();
    }
    let inline = (list.as_array_mut().into_iter()).flat_map(Array::iter_mut);
    (inline.filter_map(Value::as_inline_table_mut))
        .map(|table| -> &mut dyn TableLike { table })
        .collect()
}

/// `write` saves what the Settings window changes, and that window edits neither the Repository list nor
/// `[keys]`. Merging the list would replace the file's tables, and the user's comments on them, and merging
/// `[keys]` would write each key in Tsugiki's words over the user's: `"ctrl+k m"` over `"Ctrl+K M"`.
fn without_hand_edits(mut document: DocumentMut) -> DocumentMut {
    drop(document.remove(REPOSITORIES));
    drop(document.remove(KEYS));
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

/// Puts `command` at the head of `file`'s `recent_commands`, with the comments the user wrote on its entry,
/// and gives the commands the list holds now, as `kept_commands` reads them. Any other entry for `command`
/// goes, as does any other command written twice. Past `COMMANDS_KEPT` commands the oldest go, and so does an
/// entry that isn't text among them; one before them stays. A `recent_commands` that isn't a list gives the
/// list its place and its comments.
fn put_first_in_recent_commands(file: &mut Table, command: &CommandId) -> Vec<CommandId> {
    let mut commands = file
        .get(RECENT_COMMANDS)
        .and_then(Item::as_array)
        .cloned()
        .unwrap_or_default();
    let mut head = Value::from(command.as_str());
    let held = commands
        .iter()
        .position(|entry| entry.as_str() == Some(command.as_str()));
    if let Some(index) = held {
        *head.decor_mut() = commands.remove(index).decor().clone();
    }
    commands.insert_formatted(0, head);
    let mut kept = KeptCommands::default();
    commands.retain(|entry| match entry.as_str() {
        Some(text) => kept.keep(CommandId::new(text)),
        None => !kept.full(),
    });
    one_per_line(&mut commands);
    merge_value(
        file,
        RECENT_COMMANDS,
        &Item::Value(Value::Array(commands)),
        None,
    );
    kept.0
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

/// Hands `change` the section `key` of `file`, made first if `file` has none or has a value there that
/// isn't a table, such as a hand-written `ty = "C:/ty.exe"`, and drops the section again if `change`
/// leaves it empty.
fn edit_section(file: &mut dyn TableLike, key: &str, change: impl FnOnce(&mut dyn TableLike)) {
    if !file.get(key).is_some_and(Item::is_table_like) {
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
/// Beside the values come the problems with values Tsugiki read as their defaults, and where the file gives
/// `[appearance] theme`.
fn read_settings(
    folders: &Folders,
    machine: &Machine,
    current: &Settings,
) -> Result<(Settings, Vec<String>, String), String> {
    let path = settings_file(folders, machine);
    match read_text(&path)? {
        Some(text) => {
            let settings: Settings =
                toml_edit::de::from_str(&text).map_err(|cause| located(&path, &text, &cause))?;
            let parsed = SettingsText::parse(&path, &text);
            let place = parsed.place(APPEARANCE, "theme");
            let mut problems = parsed.unread_problems();
            problems.extend(parsed.key_problems(&settings.keys));
            Ok((settings, problems, place))
        }
        None if machine.settings_path.is_none() => {
            let place = path.display().to_string();
            Ok((Settings::default(), Vec::new(), place))
        }
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
