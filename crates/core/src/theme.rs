//! The window's colours: each **Theme** is a TOML file of tokens, parsed into typed colours here and sent to
//! the window as CSS variables (#374, #515). `docs/devel/themes.md` lists the tokens.
//!
//! A file has a `name`, a `kind` (`"light"` or `"dark"`), maybe the theme it `extends`, and `[colors]` and
//! `[syntax]` tables of tokens. An unknown key or a value that isn't a colour skips its line, and the rest of
//! the theme still loads. A file that isn't TOML, or has no `name` or `kind`, fails whole. Four themes are
//! built in, and `ThemeLibrary` adds the files of the user's `themes` folder (#519).

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::{fs, io};

use serde::{Deserialize, Serialize};
use specta::Type;
use toml_edit::{Document, TableLike};

use crate::error::{Error, ErrorKind};

/// The built-in themes, compiled in. Each sets every token.
const LIGHT_MODERN: &str = include_str!("../themes/vscode-light-modern.toml");
const DARK_MODERN: &str = include_str!("../themes/vscode-dark-modern.toml");
const GRUVBOX_LIGHT_HARD: &str = include_str!("../themes/gruvbox-light-hard.toml");
const GRUVBOX_DARK_HARD: &str = include_str!("../themes/gruvbox-dark-hard.toml");

/// Whether the window follows Windows to its light or dark side, or keeps one side whatever Windows says:
/// `[appearance] theme` in `settings.toml` (#517).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

/// A built-in theme for the light side: `[appearance] light_theme` in `settings.toml`, by its name.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LightTheme {
    #[default]
    VsCodeLightModern,
    GruvboxLightHard,
}

impl LightTheme {
    /// Every built-in light theme, in the order a list shows them.
    pub const ALL: [Self; 2] = [Self::VsCodeLightModern, Self::GruvboxLightHard];

    /// The theme's name, as its file and `settings.toml` give it.
    pub fn name(self) -> &'static str {
        match self {
            Self::VsCodeLightModern => "VS Code Light Modern",
            Self::GruvboxLightHard => "Gruvbox Light Hard",
        }
    }

    /// The theme's file, compiled in.
    pub fn file(self) -> &'static str {
        match self {
            Self::VsCodeLightModern => LIGHT_MODERN,
            Self::GruvboxLightHard => GRUVBOX_LIGHT_HARD,
        }
    }
}

/// A built-in theme for the dark side: `[appearance] dark_theme` in `settings.toml`, by its name.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DarkTheme {
    #[default]
    VsCodeDarkModern,
    GruvboxDarkHard,
}

impl DarkTheme {
    /// Every built-in dark theme, in the order a list shows them.
    pub const ALL: [Self; 2] = [Self::VsCodeDarkModern, Self::GruvboxDarkHard];

    /// The theme's name, as its file and `settings.toml` give it.
    pub fn name(self) -> &'static str {
        match self {
            Self::VsCodeDarkModern => "VS Code Dark Modern",
            Self::GruvboxDarkHard => "Gruvbox Dark Hard",
        }
    }

    /// The theme's file, compiled in.
    pub fn file(self) -> &'static str {
        match self {
            Self::VsCodeDarkModern => DARK_MODERN,
            Self::GruvboxDarkHard => GRUVBOX_DARK_HARD,
        }
    }
}

/// A theme's name, as its file and `[appearance] light_theme` and `dark_theme` give it: a built-in theme's or
/// one from the user's `themes` folder. It reaches the UI as a plain string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct ThemeName(String);

impl ThemeName {
    pub fn new(name: &str) -> Self {
        Self(name.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The name of the theme side `kind` takes unless the settings choose another.
    pub fn default_of(kind: ThemeKind) -> Self {
        match kind {
            ThemeKind::Light => Self::new(LightTheme::default().name()),
            ThemeKind::Dark => Self::new(DarkTheme::default().name()),
        }
    }
}

/// Whether a theme is for Windows' light side or its dark side. Each theme is one or the other, not a pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ThemeKind {
    Light,
    Dark,
}

impl ThemeKind {
    /// The kind as a theme file and the window write it: `"light"` or `"dark"`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        [Self::Light, Self::Dark]
            .into_iter()
            .find(|kind| kind.name() == text)
    }
}

/// The table of a theme file a token sits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenTable {
    Colors,
    Syntax,
}

impl TokenTable {
    const ALL: [Self; 2] = [Self::Colors, Self::Syntax];

    /// The table's name in a theme file.
    pub fn key(self) -> &'static str {
        match self {
            Self::Colors => "colors",
            Self::Syntax => "syntax",
        }
    }

    fn named(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|table| table.key() == key)
    }
}

/// One colour a theme sets. `docs/devel/themes.md` says what each one colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Token {
    Bg,
    Fg,
    FgMuted,
    Border,
    BgChrome,
    FgChrome,
    BgOverlay,
    Backdrop,
    Shadow,
    Selected,
    SelectionActive,
    SelectionInactive,
    FocusBorder,
    Accent,
    Add,
    Del,
    Find,
    FindCurrent,
    FindCurrentFg,
    Attention,
    Danger,
    WarnBg,
    WarnFg,
    StateOpen,
    StateDone,
    StateClosed,
    StateMuted,
    Keyword,
    String,
    Comment,
    Constant,
    Entity,
    Tag,
}

impl Token {
    /// Every token, in the order the doc page lists them.
    pub const ALL: [Self; 33] = [
        Self::Bg,
        Self::Fg,
        Self::FgMuted,
        Self::Border,
        Self::BgChrome,
        Self::FgChrome,
        Self::BgOverlay,
        Self::Backdrop,
        Self::Shadow,
        Self::Selected,
        Self::SelectionActive,
        Self::SelectionInactive,
        Self::FocusBorder,
        Self::Accent,
        Self::Add,
        Self::Del,
        Self::Find,
        Self::FindCurrent,
        Self::FindCurrentFg,
        Self::Attention,
        Self::Danger,
        Self::WarnBg,
        Self::WarnFg,
        Self::StateOpen,
        Self::StateDone,
        Self::StateClosed,
        Self::StateMuted,
        Self::Keyword,
        Self::String,
        Self::Comment,
        Self::Constant,
        Self::Entity,
        Self::Tag,
    ];

    /// The token's key in its table of a theme file.
    pub fn key(self) -> &'static str {
        match self {
            Self::Bg => "bg",
            Self::Fg => "fg",
            Self::FgMuted => "fg_muted",
            Self::Border => "border",
            Self::BgChrome => "bg_chrome",
            Self::FgChrome => "fg_chrome",
            Self::BgOverlay => "bg_overlay",
            Self::Backdrop => "backdrop",
            Self::Shadow => "shadow",
            Self::Selected => "selected",
            Self::SelectionActive => "selection_active",
            Self::SelectionInactive => "selection_inactive",
            Self::FocusBorder => "focus_border",
            Self::Accent => "accent",
            Self::Add => "add",
            Self::Del => "del",
            Self::Find => "find",
            Self::FindCurrent => "find_current",
            Self::FindCurrentFg => "find_current_fg",
            Self::Attention => "attention",
            Self::Danger => "danger",
            Self::WarnBg => "warn_bg",
            Self::WarnFg => "warn_fg",
            Self::StateOpen => "state_open",
            Self::StateDone => "state_done",
            Self::StateClosed => "state_closed",
            Self::StateMuted => "state_muted",
            Self::Keyword => "keyword",
            Self::String => "string",
            Self::Comment => "comment",
            Self::Constant => "constant",
            Self::Entity => "entity",
            Self::Tag => "tag",
        }
    }

    /// The table of a theme file the token sits in.
    pub fn table(self) -> TokenTable {
        match self {
            Self::Keyword
            | Self::String
            | Self::Comment
            | Self::Constant
            | Self::Entity
            | Self::Tag => TokenTable::Syntax,
            Self::Bg
            | Self::Fg
            | Self::FgMuted
            | Self::Border
            | Self::BgChrome
            | Self::FgChrome
            | Self::BgOverlay
            | Self::Backdrop
            | Self::Shadow
            | Self::Selected
            | Self::SelectionActive
            | Self::SelectionInactive
            | Self::FocusBorder
            | Self::Accent
            | Self::Add
            | Self::Del
            | Self::Find
            | Self::FindCurrent
            | Self::FindCurrentFg
            | Self::Attention
            | Self::Danger
            | Self::WarnBg
            | Self::WarnFg
            | Self::StateOpen
            | Self::StateDone
            | Self::StateClosed
            | Self::StateMuted => TokenTable::Colors,
        }
    }

    /// The CSS variable the window reads the token from: `fg_muted` is `--fg-muted`, and a syntax token
    /// takes `hl-` in front, so `keyword` is `--hl-keyword`.
    pub fn property(self) -> String {
        let name = self.key().replace('_', "-");
        match self.table() {
            TokenTable::Colors => format!("--{name}"),
            TokenTable::Syntax => format!("--hl-{name}"),
        }
    }

    fn named(table: TokenTable, key: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|token| token.table() == table && token.key() == key)
    }
}

/// A colour with its opacity, from `#rgb`, `#rrggbb` or `#rrggbbaa`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    red: u8,
    green: u8,
    blue: u8,
    alpha: u8,
}

impl Color {
    /// Red, green, blue and opacity, as a window's background takes them.
    pub fn rgba(self) -> [u8; 4] {
        [self.red, self.green, self.blue, self.alpha]
    }

    /// The colour as CSS writes it: `#rrggbb`, or `#rrggbbaa` when it lets something show through.
    pub fn css(self) -> String {
        let Self {
            red,
            green,
            blue,
            alpha,
        } = self;
        if alpha == u8::MAX {
            format!("#{red:02x}{green:02x}{blue:02x}")
        } else {
            format!("#{red:02x}{green:02x}{blue:02x}{alpha:02x}")
        }
    }
}

/// Text that isn't `#rgb`, `#rrggbb` or `#rrggbbaa`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotAColor;

impl FromStr for Color {
    type Err = NotAColor;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let digits = text.strip_prefix('#').ok_or(NotAColor)?;
        let nibbles = digits
            .chars()
            .map(|digit| {
                digit
                    .to_digit(16)
                    .and_then(|value| u8::try_from(value).ok())
            })
            .collect::<Option<Vec<u8>>>()
            .ok_or(NotAColor)?;
        let pair = |high: u8, low: u8| high * 16 + low;
        match *nibbles.as_slice() {
            [red, green, blue] => Ok(Self {
                red: pair(red, red),
                green: pair(green, green),
                blue: pair(blue, blue),
                alpha: u8::MAX,
            }),
            [r1, r2, g1, g2, b1, b2] => Ok(Self {
                red: pair(r1, r2),
                green: pair(g1, g2),
                blue: pair(b1, b2),
                alpha: u8::MAX,
            }),
            [r1, r2, g1, g2, b1, b2, a1, a2] => Ok(Self {
                red: pair(r1, r2),
                green: pair(g1, g2),
                blue: pair(b1, b2),
                alpha: pair(a1, a2),
            }),
            _ => Err(NotAColor),
        }
    }
}

/// Why a line of a theme file was skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// The key names no token, or no table a theme has.
    UnknownKey,
    /// The value isn't `#rgb`, `#rrggbb` or `#rrggbbaa` text, or a table isn't a table.
    BadValue,
    /// `extends` isn't text.
    NotAName,
}

/// A line of a theme file that was skipped, while the rest of the theme loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedLine {
    /// Counted from 1, when the parser knows it.
    pub line: Option<usize>,
    pub key: String,
    pub reason: SkipReason,
}

/// Why a theme file can't be used at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeError {
    /// The file isn't TOML. The line is counted from 1, when the parser knows it.
    NotToml {
        line: Option<usize>,
        message: String,
    },
    /// The file has no `name` text.
    NoName,
    /// The file's `kind` is missing, or isn't `"light"` or `"dark"`.
    NoKind,
}

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotToml {
                line: Some(line),
                message,
            } => write!(f, "line {line}: {message}"),
            Self::NotToml {
                line: None,
                message,
            } => f.write_str(message),
            Self::NoName => f.write_str("the theme has no `name`"),
            Self::NoKind => f.write_str("the theme's `kind` isn't \"light\" or \"dark\""),
        }
    }
}

/// A theme as its file gives it: the colours of the tokens it sets, the theme it builds on, and the lines it
/// skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub name: String,
    pub kind: ThemeKind,
    /// The theme whose colours it takes for the tokens it leaves out, by name. Without one, a file in the
    /// `themes` folder builds on the default theme of its kind.
    pub extends: Option<ThemeName>,
    pub colors: BTreeMap<Token, Color>,
    pub skipped: Vec<SkippedLine>,
}

impl Theme {
    /// Reads a theme file's text.
    pub fn parse(text: &str) -> Result<Self, ThemeError> {
        let document = Document::parse(text).map_err(|cause| ThemeError::NotToml {
            line: cause.span().map(|span| line_at(text, span.start)),
            message: cause.message().to_owned(),
        })?;
        let root = document.as_table();
        let mut tokens = Tokens::default();
        let mut name = None;
        let mut kind = None;
        let mut extends = None;
        for (key, item) in root.iter() {
            match key {
                "name" => name = item.as_str().map(str::to_owned),
                "kind" => kind = item.as_str().and_then(ThemeKind::parse),
                "extends" => match item.as_str() {
                    Some(base) => extends = Some(ThemeName::new(base)),
                    None => tokens.skip(key_line(text, root, key), key, SkipReason::NotAName),
                },
                _ => {
                    let line = key_line(text, root, key);
                    match TokenTable::named(key) {
                        Some(table) => match item.as_table_like() {
                            Some(given) => tokens.read(text, table, given),
                            None => tokens.skip(line, key, SkipReason::BadValue),
                        },
                        None => tokens.skip(line, key, SkipReason::UnknownKey),
                    }
                }
            }
        }
        Ok(Self {
            name: name.ok_or(ThemeError::NoName)?,
            kind: kind.ok_or(ThemeError::NoKind)?,
            extends,
            colors: tokens.colors,
            skipped: tokens.skipped,
        })
    }

    /// The theme as the window sets it on its root.
    pub fn view(&self) -> ThemeView {
        ThemeView {
            name: self.name.clone(),
            kind: self.kind,
            tokens: self
                .colors
                .iter()
                .map(|(token, color)| TokenView {
                    property: token.property(),
                    value: color.css(),
                })
                .collect(),
        }
    }
}

/// The tokens a theme file sets so far, and the lines it skipped.
#[derive(Default)]
struct Tokens {
    colors: BTreeMap<Token, Color>,
    skipped: Vec<SkippedLine>,
}

impl Tokens {
    fn read(&mut self, text: &str, table: TokenTable, given: &dyn TableLike) {
        for (key, item) in given.iter() {
            let line = key_line(text, given, key);
            let Some(token) = Token::named(table, key) else {
                self.skip(line, key, SkipReason::UnknownKey);
                continue;
            };
            match item.as_str().map(str::parse::<Color>) {
                // TOML refuses a key twice in a table, so nothing is replaced.
                Some(Ok(color)) => drop(self.colors.insert(token, color)),
                Some(Err(NotAColor)) | None => self.skip(line, key, SkipReason::BadValue),
            }
        }
    }

    fn skip(&mut self, line: Option<usize>, key: &str, reason: SkipReason) {
        self.skipped.push(SkippedLine {
            line,
            key: key.to_owned(),
            reason,
        });
    }
}

/// The line, counted from 1, of the key `key` of `table`, when the parser knows it.
fn key_line(text: &str, table: &dyn TableLike, key: &str) -> Option<usize> {
    table
        .get_key_value(key)
        .and_then(|(key, _)| key.span())
        .map(|span| line_at(text, span.start))
}

/// The line, counted from 1, that byte `offset` of `text` sits on.
fn line_at(text: &str, offset: usize) -> usize {
    let before = text.as_bytes().get(..offset).unwrap_or(text.as_bytes());
    before
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        .saturating_add(1)
}

/// A theme ready for the window: each token as a CSS variable, and the kind, which the root's
/// `color-scheme` takes so scroll bars and form controls follow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ThemeView {
    pub name: String,
    pub kind: ThemeKind,
    pub tokens: Vec<TokenView>,
}

/// A token as a CSS variable and its colour, such as `--fg-muted` and `#767676`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct TokenView {
    pub property: String,
    pub value: String,
}

/// The themes of each side, and the mode. In System mode the window shows the chosen theme of the side
/// Windows is on, and turns to the other when Windows does. Light and Dark show that side's chosen theme
/// whatever Windows says. The theme picker lists the rest (#518).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Themes {
    pub mode: ThemeMode,
    pub light: SideThemes,
    pub dark: SideThemes,
    /// What is wrong with the files of the `themes` folder, in words for the status bar, each naming its file
    /// and, when the parser knows it, its line (#519).
    pub problems: Vec<String>,
}

/// The themes of one side, and the one `settings.toml` chose for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct SideThemes {
    /// The name of the chosen theme, one of `all`.
    pub chosen: String,
    /// Every theme of the side, in the order the theme picker lists them: the built-in ones, then the files
    /// of the `themes` folder in name order.
    pub all: Vec<ThemeView>,
    /// The files of the side that can't be used, in name order, which the picker shows greyed with why.
    pub failed: Vec<FailedThemeView>,
}

/// A theme file of the `themes` folder that read as a theme, but can't be used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct FailedThemeView {
    pub name: String,
    /// The file's name in the folder, such as `solarized.toml`.
    pub file: String,
    /// Why, in words for the picker.
    pub reason: String,
}

/// Every theme Tsugiki can show: the built-in ones, then the files of the user's `themes` folder, beside the
/// settings file in use (#519). There is no watcher: the window asks for them again when it starts, when the
/// theme picker opens and on Reload themes.
///
/// A file builds on the theme it `extends`, a built-in one or another file, and without `extends` on the
/// default theme of its kind, so every theme sets every token. A theme name is taken by a built-in theme,
/// then by the files in the order of their file names: a later file with a taken name can't be used.
#[derive(Debug, Clone)]
pub struct ThemeLibrary {
    /// Each theme that can be used, with every token set: the built-in ones in their order, then the files'
    /// in name order.
    usable: Vec<Theme>,
    /// In name order.
    failed: Vec<FailedTheme>,
    /// Lines the files skipped, and files that aren't themes, in words for the status bar.
    problems: Vec<String>,
}

/// A theme file that read as a theme, but can't be used.
#[derive(Debug, Clone)]
struct FailedTheme {
    name: String,
    kind: ThemeKind,
    file: String,
    why: ThemeFailure,
}

/// Why a theme file that read as a theme can't be used.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ThemeFailure {
    /// A built-in theme has its name.
    BuiltInName,
    /// The file named, by its file name, has its name and comes first.
    NameTaken(String),
    /// Following `extends` comes back to it, through the names given, it first and last.
    Loop(Vec<ThemeName>),
    /// It extends the name given, which no theme has.
    NoSuchTheme(ThemeName),
    /// It extends the theme named, which is for the other side.
    OtherSide(ThemeName, ThemeKind),
    /// It extends the theme named, which can't be used.
    BaseFailed(ThemeName),
}

impl FailedTheme {
    fn view(&self) -> FailedThemeView {
        let reason = match &self.why {
            ThemeFailure::BuiltInName => "a built-in theme has this name".to_owned(),
            ThemeFailure::NameTaken(first) => format!("{first} names a theme {} first", self.name),
            ThemeFailure::Loop(names) => {
                let names: Vec<&str> = names.iter().map(ThemeName::as_str).collect();
                format!("it extends itself: {}", names.join(", "))
            }
            ThemeFailure::NoSuchTheme(base) => {
                format!("it extends {}, which no theme is named", base.as_str())
            }
            ThemeFailure::OtherSide(base, kind) => {
                format!("it extends {}, a {} theme", base.as_str(), kind.name())
            }
            ThemeFailure::BaseFailed(base) => {
                format!("it extends {}, which can't be used", base.as_str())
            }
        };
        FailedThemeView {
            name: self.name.clone(),
            file: self.file.clone(),
            reason,
        }
    }
}

impl ThemeLibrary {
    /// The built-in themes and the files of `folder`. A folder that isn't there holds no theme.
    pub fn read(folder: &Path) -> Result<Self, Error> {
        let built_in: Vec<Theme> = LightTheme::ALL
            .map(LightTheme::file)
            .into_iter()
            .chain(DarkTheme::ALL.map(DarkTheme::file))
            .map(Themes::built_in)
            .collect::<Result<_, _>>()?;
        let mut problems = Vec::new();
        let mut files = Vec::new();
        for (path, text) in theme_files(folder, &mut problems) {
            match Theme::parse(&text) {
                Ok(theme) => {
                    problems.extend(theme.skipped.iter().map(|line| skipped(&path, line)));
                    let file = path.file_name().unwrap_or_default().to_string_lossy();
                    files.push((file.into_owned(), theme));
                }
                Err(cause) => problems.push(not_a_theme(&path, &cause)),
            }
        }
        let (mut usable, mut failed) = resolve(&built_in, &files);
        let by_name = |name: &str| name.to_lowercase();
        usable.sort_by_cached_key(|theme| by_name(&theme.name));
        failed.sort_by_cached_key(|theme| (by_name(&theme.name), theme.file.clone()));
        Ok(Self {
            usable: built_in.into_iter().chain(usable).collect(),
            failed,
            problems,
        })
    }

    /// The name of the usable theme of side `kind` named `name`, as the theme picker saves it.
    pub fn named(&self, kind: ThemeKind, name: &str) -> Result<ThemeName, Error> {
        let found = self
            .usable
            .iter()
            .find(|theme| theme.kind == kind && theme.name == name);
        found
            .map(|theme| ThemeName::new(&theme.name))
            .ok_or_else(|| {
                let message = format!("No {} theme is named {name:?}.", kind.name());
                Error::new(ErrorKind::NotFound, &message)
            })
    }

    /// The themes for the window, with `light` and `dark` chosen. A name no usable theme of its side has, as
    /// when its file has a mistake or has gone, gives way to the side's default, and the problems say so.
    pub fn themes(&self, mode: ThemeMode, light: &ThemeName, dark: &ThemeName) -> Themes {
        let mut problems = self.problems.clone();
        let light = self.side(ThemeKind::Light, light, &mut problems);
        let dark = self.side(ThemeKind::Dark, dark, &mut problems);
        Themes {
            mode,
            light,
            dark,
            problems,
        }
    }

    fn side(&self, kind: ThemeKind, chosen: &ThemeName, problems: &mut Vec<String>) -> SideThemes {
        let all: Vec<ThemeView> = self
            .usable
            .iter()
            .filter(|theme| theme.kind == kind)
            .map(Theme::view)
            .collect();
        let held = all.iter().any(|theme| theme.name == chosen.as_str());
        let chosen = if held {
            chosen.clone()
        } else {
            let default = ThemeName::default_of(kind);
            let (side, missing, shown) = (kind.name(), chosen.as_str(), default.as_str());
            problems.push(format!(
                "No {side} theme is named {missing:?}, so Tsugiki shows {shown}."
            ));
            default
        };
        SideThemes {
            chosen: chosen.0,
            all,
            failed: self
                .failed
                .iter()
                .filter(|theme| theme.kind == kind)
                .map(FailedTheme::view)
                .collect(),
        }
    }
}

/// The path and the text of each `.toml` file of `folder`, in the order of their names. A folder that isn't
/// there holds none. One that can't be listed, or a file that can't be read, joins `problems`.
fn theme_files(folder: &Path, problems: &mut Vec<String>) -> Vec<(PathBuf, String)> {
    let is_toml = |path: &PathBuf| {
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
    };
    let listed = fs::read_dir(folder).and_then(Iterator::collect::<Result<Vec<_>, _>>);
    let entries = match listed {
        Ok(entries) => entries,
        Err(cause) if cause.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(cause) => {
            problems.push(format!("Tsugiki can't read {}: {cause}", folder.display()));
            Vec::new()
        }
    };
    let mut paths: Vec<PathBuf> = entries
        .iter()
        .map(fs::DirEntry::path)
        .filter(is_toml)
        .collect();
    paths.sort();
    let mut files = Vec::new();
    for path in paths {
        match fs::read_to_string(&path) {
            Ok(text) => files.push((path, text)),
            Err(cause) => problems.push(format!("Tsugiki can't read {}: {cause}", path.display())),
        }
    }
    files
}

/// A line `path` skipped, in words for the status bar.
fn skipped(path: &Path, line: &SkippedLine) -> String {
    let at = line
        .line
        .map_or_else(String::new, |line| format!(", line {line}"));
    let why = match line.reason {
        SkipReason::UnknownKey => "names no token",
        SkipReason::BadValue => "isn't a colour",
        SkipReason::NotAName => "isn't a theme's name",
    };
    format!(
        "{}{at}: {} {why}, so Tsugiki skips it.",
        path.display(),
        line.key
    )
}

/// Why `path` isn't a theme, in words for the status bar.
fn not_a_theme(path: &Path, cause: &ThemeError) -> String {
    match cause {
        ThemeError::NotToml {
            line: Some(line),
            message,
        } => format!("{}, line {line}: {message}", path.display()),
        other @ (ThemeError::NotToml { line: None, .. }
        | ThemeError::NoName
        | ThemeError::NoKind) => format!("{}: {other}", path.display()),
    }
}

/// The themes of `files` that can be used, with every token set, and the ones that can't. `files` holds each
/// file's name and theme, in the order of the file names, which is the order they take theme names in.
fn resolve(built_in: &[Theme], files: &[(String, Theme)]) -> (Vec<Theme>, Vec<FailedTheme>) {
    let mut failed = Vec::new();
    // The file that took each theme name first.
    let mut first: BTreeMap<&str, &str> = BTreeMap::new();
    let mut owners = Vec::new();
    for (file, theme) in files {
        let taken = if built_in.iter().any(|built| built.name == theme.name) {
            Some(ThemeFailure::BuiltInName)
        } else {
            let earlier = first.get(theme.name.as_str());
            earlier.map(|&earlier| ThemeFailure::NameTaken(earlier.to_owned()))
        };
        match taken {
            Some(why) => failed.push(FailedTheme::of(file, theme, why)),
            None => {
                let _ = first.insert(&theme.name, file);
                owners.push((file, theme));
            }
        }
    }
    let resolver = Resolver {
        built_in,
        named: owners
            .iter()
            .map(|&(_, theme)| (theme.name.as_str(), theme))
            .collect(),
    };
    let mut usable = Vec::new();
    for (file, theme) in owners {
        match resolver.colors(theme, &mut Vec::new()) {
            Ok(colors) => usable.push(Theme {
                colors,
                ..theme.clone()
            }),
            Err(why) => failed.push(FailedTheme::of(file, theme, why)),
        }
    }
    (usable, failed)
}

impl FailedTheme {
    fn of(file: &str, theme: &Theme, why: ThemeFailure) -> Self {
        Self {
            name: theme.name.clone(),
            kind: theme.kind,
            file: file.to_owned(),
            why,
        }
    }
}

/// Works out a file's colours from the theme it extends.
struct Resolver<'a> {
    built_in: &'a [Theme],
    /// The file's theme that took each theme name first.
    named: BTreeMap<&'a str, &'a Theme>,
}

impl<'a> Resolver<'a> {
    /// Every token's colour for `theme`: its own, and the rest from the theme it builds on. `through` holds
    /// the names of the files followed to get here, so a loop back to one of them is found.
    fn colors(
        &self,
        theme: &'a Theme,
        through: &mut Vec<&'a str>,
    ) -> Result<BTreeMap<Token, Color>, ThemeFailure> {
        if let Some(start) = through.iter().position(|&name| name == theme.name) {
            let mut names: Vec<ThemeName> = through
                .iter()
                .skip(start)
                .map(|&name| ThemeName::new(name))
                .collect();
            names.push(ThemeName::new(&theme.name));
            return Err(ThemeFailure::Loop(names));
        }
        let default = ThemeName::default_of(theme.kind);
        let base = theme.extends.as_ref().unwrap_or(&default);
        let built = self
            .built_in
            .iter()
            .find(|built| built.name == base.as_str());
        let found = built.or_else(|| self.named.get(base.as_str()).copied());
        let base_theme = found.ok_or_else(|| ThemeFailure::NoSuchTheme(base.clone()))?;
        if base_theme.kind != theme.kind {
            return Err(ThemeFailure::OtherSide(base.clone(), base_theme.kind));
        }
        let mut colors = match built {
            Some(built) => built.colors.clone(),
            None => {
                through.push(&theme.name);
                let found = self.colors(base_theme, through);
                let _ = through.pop();
                let in_loop =
                    |names: &[ThemeName]| names.iter().any(|name| name.as_str() == theme.name);
                // A loop through this file fails it with the loop; any other failure is its base's.
                found.map_err(|why| match why {
                    ThemeFailure::Loop(names) if in_loop(&names) => ThemeFailure::Loop(names),
                    ThemeFailure::Loop(_)
                    | ThemeFailure::BuiltInName
                    | ThemeFailure::NameTaken(_)
                    | ThemeFailure::NoSuchTheme(_)
                    | ThemeFailure::OtherSide(..)
                    | ThemeFailure::BaseFailed(_) => ThemeFailure::BaseFailed(base.clone()),
                })?
            }
        };
        colors.extend(theme.colors.iter().map(|(&token, &color)| (token, color)));
        Ok(colors)
    }
}

impl Themes {
    /// The `bg` of the built-in theme of side `kind`, which the window takes before it first shows, so a
    /// dark theme doesn't flash white while the page loads (#516).
    #[expect(
        clippy::expect_used,
        reason = "the built-in files are compiled in, and a test reads the `bg` of each"
    )]
    pub fn built_in_background(kind: ThemeKind) -> Color {
        let text = match kind {
            ThemeKind::Light => LIGHT_MODERN,
            ThemeKind::Dark => DARK_MODERN,
        };
        Theme::parse(text)
            .ok()
            .and_then(|theme| theme.colors.get(&Token::Bg).copied())
            .expect("each built-in theme sets bg")
    }

    /// The built-in theme the file `text` holds. It must set every token, so the window turning from one
    /// theme to another leaves no colour behind.
    fn built_in(text: &str) -> Result<Theme, Error> {
        let refused = |why: &str| {
            Error::new(
                ErrorKind::Internal,
                &format!("A built-in theme can't be read: {why}"),
            )
        };
        let theme = Theme::parse(text).map_err(|cause| refused(&cause.to_string()))?;
        let missing: Vec<&str> = Token::ALL
            .into_iter()
            .filter(|token| !theme.colors.contains_key(token))
            .map(Token::key)
            .collect();
        if missing.is_empty() {
            Ok(theme)
        } else {
            Err(refused(&format!(
                "{} sets no {}",
                theme.name,
                missing.join(", ")
            )))
        }
    }
}

#[cfg(test)]
mod tests;
