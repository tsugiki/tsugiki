//! The window's colours: each **Theme** is a TOML file of tokens, parsed into typed colours here and sent to
//! the window as CSS variables (#374, #515). `docs/devel/themes.md` lists the tokens.
//!
//! A file has a `name`, a `kind` (`"light"` or `"dark"`), and `[colors]` and `[syntax]` tables of tokens. An
//! unknown key or a value that isn't a colour skips its line, and the rest of the theme still loads. A file
//! that isn't TOML, or has no `name` or `kind`, fails whole.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
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

    /// The light theme named `name`, written as `name()` writes it.
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|theme| theme.name() == name)
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

    /// The dark theme named `name`, written as `name()` writes it.
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|theme| theme.name() == name)
    }

    /// The theme's file, compiled in.
    pub fn file(self) -> &'static str {
        match self {
            Self::VsCodeDarkModern => DARK_MODERN,
            Self::GruvboxDarkHard => GRUVBOX_DARK_HARD,
        }
    }
}

/// A theme's name in a settings file, and the error a name no theme of the side has gives.
macro_rules! by_name {
    ($side:ty, $what:literal) => {
        impl Serialize for $side {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.name())
            }
        }

        impl<'de> Deserialize<'de> for $side {
            fn deserialize<D: Deserializer<'de>>(given: D) -> Result<Self, D::Error> {
                let name = String::deserialize(given)?;
                Self::named(&name).ok_or_else(|| {
                    serde::de::Error::custom(format!("no {} is named {name:?}", $what))
                })
            }
        }
    };
}

by_name!(LightTheme, "light theme");
by_name!(DarkTheme, "dark theme");

/// A built-in theme of either side, as the theme picker saves it to `light_theme` or `dark_theme` (#518).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeChoice {
    Light(LightTheme),
    Dark(DarkTheme),
}

impl ThemeChoice {
    /// The theme of side `kind` named `name`, as the window sends it.
    pub fn named(kind: ThemeKind, name: &str) -> Result<Self, Error> {
        let found = match kind {
            ThemeKind::Light => LightTheme::named(name).map(Self::Light),
            ThemeKind::Dark => DarkTheme::named(name).map(Self::Dark),
        };
        found.ok_or_else(|| {
            Error::new(
                ErrorKind::NotFound,
                &format!("No {} theme is named {name:?}.", kind.name()),
            )
        })
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
    fn name(self) -> &'static str {
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

/// A theme as its file gives it: the colours of the tokens it sets, and the lines it skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub name: String,
    pub kind: ThemeKind,
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
        for (key, item) in root.iter() {
            match key {
                "name" => name = item.as_str().map(str::to_owned),
                "kind" => kind = item.as_str().and_then(ThemeKind::parse),
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
}

/// The themes of one side, and the one `settings.toml` chose for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct SideThemes {
    /// The name of the chosen theme, one of `all`.
    pub chosen: String,
    /// Every theme of the side, in the order the theme picker lists them.
    pub all: Vec<ThemeView>,
}

impl Themes {
    /// Every built-in theme of each side, with the ones `settings.toml`'s `[appearance]` chose.
    pub fn chosen(mode: ThemeMode, light: LightTheme, dark: DarkTheme) -> Result<Self, Error> {
        let side = |chosen: &str, files: &[&str]| -> Result<SideThemes, Error> {
            Ok(SideThemes {
                chosen: chosen.to_owned(),
                all: files
                    .iter()
                    .map(|file| Self::built_in(file))
                    .collect::<Result<_, _>>()?,
            })
        };
        Ok(Self {
            mode,
            light: side(light.name(), &LightTheme::ALL.map(LightTheme::file))?,
            dark: side(dark.name(), &DarkTheme::ALL.map(DarkTheme::file))?,
        })
    }

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
    fn built_in(text: &str) -> Result<ThemeView, Error> {
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
            Ok(theme.view())
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
