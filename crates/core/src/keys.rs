//! Tsugiki's keys: each action the user can bind, its title, its default key and where it works (#548). The
//! window binds actions, not keys, and finds a key's action in this table, which `npm run bindings` writes
//! to `ui/src/lib/data/keymap.ts`. The user gives an action another key in `[keys]` in settings.toml (#549),
//! which names each action as serde names it here, and `Remaps` reads those lines into typed keys.

use std::fmt;

use serde::de::IntoDeserializer;
use serde::de::value::{Error as NameError, StrDeserializer};
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};
use specta::Type;

/// Where an action's key works. A key that works anywhere shares every other context, and the composer
/// shares the diff's, as an inline comment's composer sits in the diff, whose keys with a modifier work in
/// it too.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Context {
    Anywhere,
    IssueList,
    Diff,
    Composer,
}

impl Context {
    /// Whether a key can mean something in both contexts at once.
    fn overlaps(self, other: Self) -> bool {
        match (self, other) {
            (Self::Anywhere, _)
            | (_, Self::Anywhere)
            | (Self::Diff, Self::Composer)
            | (Self::Composer, Self::Diff) => true,
            (Self::IssueList | Self::Diff | Self::Composer, _) => self == other,
        }
    }
}

/// The modifiers held with a key. Ctrl is Cmd on macOS, as `docs/devel/keyboard.md` has it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Modifiers {
    Plain,
    Shift,
    Ctrl,
    Alt,
    CtrlShift,
    CtrlAlt,
    AltShift,
    CtrlAltShift,
}

impl Modifiers {
    /// The modifiers a press names before its key, "ctrl+shift" in "ctrl+shift+p", in any order and case,
    /// with Cmd for Ctrl and Option for Alt as macOS calls them. None for a word that names no modifier, or
    /// names one twice.
    fn parse(held: &str) -> Option<Self> {
        let (mut ctrl, mut alt, mut shift) = (false, false, false);
        for word in held.split('+') {
            let flag = match word.to_lowercase().as_str() {
                "ctrl" | "cmd" => &mut ctrl,
                "alt" | "option" => &mut alt,
                "shift" => &mut shift,
                _ => return None,
            };
            if *flag {
                return None;
            }
            *flag = true;
        }
        Some(match (ctrl, alt, shift) {
            (false, false, false) => Self::Plain,
            (false, false, true) => Self::Shift,
            (true, false, false) => Self::Ctrl,
            (false, true, false) => Self::Alt,
            (true, false, true) => Self::CtrlShift,
            (true, true, false) => Self::CtrlAlt,
            (false, true, true) => Self::AltShift,
            (true, true, true) => Self::CtrlAltShift,
        })
    }

    /// Whether Ctrl or Alt is held, which takes a key away from typing and moving around.
    fn take_the_key(self) -> bool {
        match self {
            Self::Plain | Self::Shift => false,
            Self::Ctrl
            | Self::Alt
            | Self::CtrlShift
            | Self::CtrlAlt
            | Self::AltShift
            | Self::CtrlAltShift => true,
        }
    }

    /// Whether Shift is held.
    fn shift(self) -> bool {
        match self {
            Self::Plain | Self::Ctrl | Self::Alt | Self::CtrlAlt => false,
            Self::Shift | Self::CtrlShift | Self::AltShift | Self::CtrlAltShift => true,
        }
    }

    /// The modifiers as the window's `keyName` writes them before a key, in the order Ctrl, Alt, Shift.
    fn prefix(self) -> &'static str {
        match self {
            Self::Plain => "",
            Self::Shift => "shift+",
            Self::Ctrl => "ctrl+",
            Self::Alt => "alt+",
            Self::CtrlShift => "ctrl+shift+",
            Self::CtrlAlt => "ctrl+alt+",
            Self::AltShift => "alt+shift+",
            Self::CtrlAltShift => "ctrl+alt+shift+",
        }
    }
}

/// Writes `Named`, a key that types nothing, with the name the window's `keyName` gives it.
macro_rules! named {
    ($($key:ident $name:literal,)*) => {
        /// A key that types nothing, such as F7 or Page Down.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum Named {
            $($key,)*
        }

        impl Named {
            const ALL: &[Self] = &[$(Self::$key,)*];

            /// The key's name, as the window's `keyName` gives it: `KeyboardEvent.key` in lower case.
            fn name(self) -> &'static str {
                match self {
                    $(Self::$key => $name,)*
                }
            }
        }
    };
}

named! {
    ArrowLeft "arrowleft",
    ArrowRight "arrowright",
    ArrowUp "arrowup",
    ArrowDown "arrowdown",
    PageUp "pageup",
    PageDown "pagedown",
    Home "home",
    End "end",
    Enter "enter",
    Escape "escape",
    Tab "tab",
    Space "space",
    Backspace "backspace",
    Delete "delete",
    Insert "insert",
    F1 "f1",
    F2 "f2",
    F3 "f3",
    F4 "f4",
    F5 "f5",
    F6 "f6",
    F7 "f7",
    F8 "f8",
    F9 "f9",
    F10 "f10",
    F11 "f11",
    F12 "f12",
    F13 "f13",
    F14 "f14",
    F15 "f15",
    F16 "f16",
    F17 "f17",
    F18 "f18",
    F19 "f19",
    F20 "f20",
    F21 "f21",
    F22 "f22",
    F23 "f23",
    F24 "f24",
}

/// Other names a user may write for a key: the arrows as `docs/devel/keyboard.md` draws them, and Esc.
const ALIASES: &[(&str, Named)] = &[
    ("←", Named::ArrowLeft),
    ("→", Named::ArrowRight),
    ("↑", Named::ArrowUp),
    ("↓", Named::ArrowDown),
    ("esc", Named::Escape),
];

/// The keys that move around, confirm or press a button, in the window and in a text box, which no action
/// takes from them unless Ctrl or Alt is held (#370).
const OWN_WORK: &[Named] = &[
    Named::Space,
    Named::ArrowLeft,
    Named::ArrowRight,
    Named::ArrowUp,
    Named::ArrowDown,
    Named::PageUp,
    Named::PageDown,
    Named::Home,
    Named::End,
    Named::Enter,
    Named::Escape,
    Named::Tab,
];

impl Named {
    /// The key named `lower`, a name in lower case.
    fn parse(lower: &str) -> Option<Self> {
        let named = Self::ALL.iter().find(|named| named.name() == lower);
        let alias = || ALIASES.iter().find(|(alias, _)| *alias == lower);
        named.copied().or_else(|| alias().map(|(_, named)| *named))
    }
}

/// A key on the keyboard: what it types, or its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Key {
    /// A key that types, by what it types, in lower case: "p", "?", ",".
    Char(char),
    Named(Named),
}

impl Key {
    /// The key `text` names, in any case: "P", "PageDown", "←".
    fn parse(text: &str) -> Option<Self> {
        let lower = text.to_lowercase();
        if let Some(named) = Named::parse(&lower) {
            return Some(Self::Named(named));
        }
        let mut chars = lower.chars();
        match (chars.next(), chars.next()) {
            (Some(typed), None) if !typed.is_whitespace() && !typed.is_control() => {
                Some(Self::Char(typed))
            }
            _ => None,
        }
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Char(typed) => write!(f, "{typed}"),
            Self::Named(named) => f.write_str(named.name()),
        }
    }
}

/// One press: a key and the modifiers held with it. It reads as the window's `keyName` names a press,
/// "ctrl+shift+p", with the key in lower case and the modifiers in the order Ctrl, Alt, Shift.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Press {
    modifiers: Modifiers,
    key: Key,
}

impl Press {
    /// The press `text` names: "ctrl+shift+p", "Alt+PageDown", "ctrl++" for Ctrl and the plus key.
    fn parse(text: &str) -> Option<Self> {
        let (held, key) = match text.strip_suffix("++") {
            Some(held) => (Some(held), "+"),
            None if text == "+" => (None, text),
            None => text
                .rsplit_once('+')
                .map_or((None, text), |(held, key)| (Some(held), key)),
        };
        Some(Self {
            modifiers: held.map_or(Some(Modifiers::Plain), Modifiers::parse)?,
            key: Key::parse(key)?,
        })
    }

    /// Whether the press moves around, confirms or presses a button, which an action can't take from it
    /// (#370).
    fn keeps_its_work(self) -> bool {
        let working = match self.key {
            Key::Named(named) => OWN_WORK.contains(&named),
            Key::Char(_) => false,
        };
        working && !self.modifiers.take_the_key()
    }

    /// The symbol or digit pressed with Shift, which the window never names that way: it names the key by
    /// what Shift makes it type, "?" and not "shift+/". Only a Latin letter keeps its Shift. A key the window
    /// names by its place, as with macOS's Option or on a non-Latin layout, takes what Shift types there on a
    /// US keyboard: Option+Shift+/ is "alt+?".
    fn shifted_symbol(self) -> Option<char> {
        match self.key {
            Key::Char(typed) if self.modifiers.shift() && !typed.is_ascii_lowercase() => {
                Some(typed)
            }
            Key::Char(_) | Key::Named(_) => None,
        }
    }
}

/// The press on which WebView2 opens DevTools in a development build, before Tsugiki sees it (ADR 0013): no
/// key can start with it.
const DEVTOOLS: Press = Press {
    modifiers: Modifiers::CtrlShift,
    key: Key::Char('i'),
};

/// What Shift makes `typed` type on a US keyboard, for a problem to suggest.
fn shifted_on_us_keyboard(typed: char) -> Option<char> {
    const PAIRS: &[(char, char)] = &[
        ('1', '!'),
        ('2', '@'),
        ('3', '#'),
        ('4', '$'),
        ('5', '%'),
        ('6', '^'),
        ('7', '&'),
        ('8', '*'),
        ('9', '('),
        ('0', ')'),
        ('-', '_'),
        ('=', '+'),
        ('[', '{'),
        (']', '}'),
        ('\\', '|'),
        (';', ':'),
        ('\'', '"'),
        ('`', '~'),
        (',', '<'),
        ('.', '>'),
        ('/', '?'),
    ];
    PAIRS
        .iter()
        .find(|(key, _)| *key == typed)
        .map(|(_, shifted)| *shifted)
}

impl fmt::Display for Press {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.modifiers.prefix(), self.key)
    }
}

/// An action's key: one press, or a chord of two, such as `Ctrl+K M`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keys {
    One(Press),
    Chord(Press, Press),
}

impl Keys {
    /// The key `text` names: one press, or two apart, as `[keys]` and `docs/devel/keyboard.md` write them.
    /// The case doesn't count: "Ctrl+K M" is "ctrl+k m".
    pub fn parse(text: &str) -> Option<Self> {
        let mut presses = text.split_whitespace().map(Press::parse);
        match (presses.next(), presses.next(), presses.next()) {
            (Some(one), None, None) => Some(Self::One(one?)),
            (Some(first), Some(second), None) => Some(Self::Chord(first?, second?)),
            _ => None,
        }
    }

    /// Whether the two can't both stand: the same key, or one press that starts the other's chord, which
    /// would then never wait for its second key.
    fn clashes(self, other: Self) -> bool {
        match (self, other) {
            (Self::One(press), Self::Chord(first, _))
            | (Self::Chord(first, _), Self::One(press)) => press == first,
            (Self::One(_), Self::One(_)) | (Self::Chord(..), Self::Chord(..)) => self == other,
        }
    }

    /// The key's one press, or its chord's first.
    const fn first(self) -> Press {
        match self {
            Self::One(first) | Self::Chord(first, _) => first,
        }
    }

    /// Whether the key's first press moves around, confirms or presses a button, so no action can take it.
    /// A chord's second press can: the chord's wait keeps it from the browser and from every other handler.
    fn keeps_its_work(self) -> bool {
        self.first().keeps_its_work()
    }

    /// The first symbol or digit of the key pressed with Shift, which the window never names that way.
    fn shifted_symbol(self) -> Option<char> {
        match self {
            Self::One(press) => press.shifted_symbol(),
            Self::Chord(first, second) => {
                first.shifted_symbol().or_else(|| second.shifted_symbol())
            }
        }
    }
}

/// A key as the window's `keyName` and `chordName` name it: "ctrl+shift+p", or a chord's two presses joined
/// by a space, "ctrl+k m".
impl fmt::Display for Keys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::One(press) => write!(f, "{press}"),
            Self::Chord(first, second) => write!(f, "{first} {second}"),
        }
    }
}

impl Serialize for Keys {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// One action of the table: its title on the Keyboard page, its default key, and where that key works.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Binding {
    pub action: Action,
    pub title: &'static str,
    /// None for an action only the command palette runs until the user gives it a key.
    pub key: Option<Keys>,
    pub context: Context,
}

impl Binding {
    /// Whether the two actions can't both keep their keys: they share a context, and one's key is the
    /// other's, or starts the other's chord.
    pub fn clashes(&self, other: &Self) -> bool {
        match (self.key, other.key) {
            (Some(mine), Some(theirs)) => {
                self.context.overlaps(other.context) && mine.clashes(theirs)
            }
            (None, _) | (_, None) => false,
        }
    }

    /// The end of a problem with a line of `[keys]`: the action keeps its default key.
    fn kept(&self) -> String {
        match self.key {
            Some(key) => format!(r#"so {} keeps "{key}"."#, self.title),
            None => format!("so {} keeps no key.", self.title),
        }
    }
}

/// An action's default key, written in `table!` as its modifiers and key, two of those for a chord, or
/// nothing for no key. A key that types is a `char`, and one that types nothing names its `Named`. A macro
/// and not a `const fn`, which would run only as the code compiles, where the coverage gate can't see it.
macro_rules! keys {
    () => {
        None
    };
    ($held:ident $key:tt) => {
        Some(Keys::One(press!($held $key)))
    };
    ($held:ident $key:tt, $then_held:ident $then:tt) => {
        Some(Keys::Chord(press!($held $key), press!($then_held $then)))
    };
}

/// One press of `keys!`.
macro_rules! press {
    ($held:ident $key:literal) => {
        Press {
            modifiers: Modifiers::$held,
            key: Key::Char($key),
        }
    };
    ($held:ident $key:ident) => {
        Press {
            modifiers: Modifiers::$held,
            key: Key::Named(Named::$key),
        }
    };
}

/// Writes the `Action` enum and `TABLE` from one list, so an action can't miss its row or have two.
macro_rules! table {
    ($($action:ident: $title:literal, ($($key:tt)*), $context:ident;)*) => {
        /// An action the user can bind a key to, named in `[keys]` as serde names it: `archive`,
        /// `local_mark`.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
        #[serde(rename_all = "snake_case")]
        pub enum Action {
            $($action,)*
        }

        impl Action {
            /// The action's row of `TABLE`.
            pub fn binding(self) -> Binding {
                match self {
                    $(Self::$action => Binding {
                        action: self,
                        title: $title,
                        key: keys!($($key)*),
                        context: Context::$context,
                    },)*
                }
            }
        }

        /// Every action, in the order the Keyboard page lists them, which is `docs/devel/keyboard.md`'s.
        pub const TABLE: &[Binding] = &[
            $(Binding {
                action: Action::$action,
                title: $title,
                key: keys!($($key)*),
                context: Context::$context,
            },)*
        ];
    };
}

table! {
    CommandPalette: "Command Palette", (CtrlShift 'p'), Anywhere;
    QuickOpen: "Quick Open", (Ctrl 'p'), Anywhere;
    GoToDefinition: "Go to Definition", (Plain F12), Diff;
    PeekDefinition: "Peek Definition", (Alt F12), Diff;
    NextChange: "Next Change", (Plain F7), Diff;
    PreviousChange: "Previous Change", (Shift F7), Diff;
    NextThread: "Next Comment Thread", (Plain F8), Diff;
    PreviousThread: "Previous Comment Thread", (Shift F8), Diff;
    FindInDiff: "Find in Diff", (Ctrl 'f'), Diff;
    ToggleSidebar: "Hide or Show the Sidebar", (Ctrl 'b'), Anywhere;
    ToggleIssueList: "Hide or Show the Issue List", (CtrlAlt 'b'), Anywhere;
    Unfold: "Unfold an Issue's PRs", (Plain ArrowRight), IssueList;
    Fold: "Fold an Issue's PRs", (Plain ArrowLeft), IssueList;
    ToggleDetailOnly: "Show Only the Detail Pane, or the Panes", (Ctrl 'k', Plain 'z'), Anywhere;
    ToggleDiffView: "Unified or Side by Side", (CtrlAlt 'u'), Diff;
    ToggleFilesMode: "All Files or One File at a Time", (Ctrl 'k', Plain 'o'), Diff;
    UnfoldUnchanged: "Unfold Unchanged Files", (Ctrl 'k', Plain 'j'), Diff;
    NextTab: "Next Tab", (Ctrl PageDown), Anywhere;
    PreviousTab: "Previous Tab", (Ctrl PageUp), Anywhere;
    NextFile: "Next File", (Alt PageDown), Diff;
    PreviousFile: "Previous File", (Alt PageUp), Diff;
    SendComment: "Send Comment", (Ctrl Enter), Composer;
    OpenInEditor: "Open in Editor", (CtrlShift 'e'), Diff;
    LocalMark: "Local Mark", (Ctrl 'k', Plain 'm'), Anywhere;
    Pin: "Pin or Unpin", (Ctrl 'k', Plain 'p'), Anywhere;
    PickTheme: "Pick a Theme", (Ctrl 'k', Plain 't'), Anywhere;
    ReloadThemes: "Reload Themes", (Ctrl 'k', Plain 'r'), Anywhere;
    GoBack: "Go Back", (Alt ArrowLeft), Anywhere;
    GoForward: "Go Forward", (Alt ArrowRight), Anywhere;
    OpenOnGithub: "Open on GitHub", (CtrlShift 'g'), Anywhere;
    SwitchRepository: "Switch Repository", (Ctrl 'r'), Anywhere;
    ResetLayout: "Reset Layout", (), Anywhere;
    Archive: "Archive", (Plain 'e'), IssueList;
    Mute: "Mute", (Plain 'm'), IssueList;
    MarkSeen: "Mark Seen", (Plain 'q'), Anywhere;
    MarkUnseen: "Mark Unseen", (Shift 'q'), Anywhere;
    OpenSettings: "Open Settings", (Ctrl ','), Anywhere;
    ZoomIn: "Zoom In", (Ctrl '='), Anywhere;
    ZoomOut: "Zoom Out", (Ctrl '-'), Anywhere;
    ZoomReset: "Reset Zoom", (Ctrl '0'), Anywhere;
    ToggleHud: "Show or Hide the Performance HUD", (CtrlShift 'h'), Anywhere;
    OpenDevtools: "Open DevTools", (CtrlShift 'i'), Anywhere;
    ShowKeys: "List the View's Keys", (Plain '?'), Anywhere;
}

impl Action {
    /// The action `name` names in `[keys]`, such as `local_mark`.
    fn parse(name: &str) -> Option<Self> {
        let name: StrDeserializer<'_, NameError> = name.into_deserializer();
        Self::deserialize(name).ok()
    }
}

/// A line of `[keys]`: the action it names, and the key it gives that action in place of the default, or
/// none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Remap {
    pub action: Action,
    pub key: Option<Keys>,
}

impl Remap {
    /// The line `name = value` of `[keys]`, with no `value` for one that isn't text. A problem comes back in
    /// words for the status bar, which follow the line's name.
    fn read(name: &str, value: Option<&str>) -> Result<Self, String> {
        let action = Action::parse(name)
            .ok_or_else(|| "isn't an action Tsugiki has, so Tsugiki skips the line.".to_owned())?;
        let binding = action.binding();
        let refused = |problem: String| format!("{problem}, {}", binding.kept());
        if action == Action::OpenDevtools {
            let problem = "can't be remapped, as WebView2 opens DevTools on its own Ctrl+Shift+I in a \
                           development build";
            return Err(refused(problem.to_owned()));
        }
        let Some(text) = value else {
            return Err(refused(
                r#"needs a key in quotes, such as "ctrl+k m""#.to_owned(),
            ));
        };
        if text.is_empty() {
            return Ok(Self { action, key: None });
        }
        let key = Self::key(text, binding).map_err(refused)?;
        Ok(Self {
            action,
            key: Some(key),
        })
    }

    /// The key `text` gives the action of `binding`, or what is wrong with it.
    fn key(text: &str, binding: Binding) -> Result<Keys, String> {
        let key = Keys::parse(text)
            .ok_or_else(|| format!(r#"gives "{text}", which Tsugiki can't read as a key"#))?;
        if let Some(typed) = key.shifted_symbol() {
            let instead = shifted_on_us_keyboard(typed).map_or_else(
                || format!("what Shift+{typed} types"),
                |shifted| format!(r#""{shifted}", which Shift+{typed} types on a US keyboard"#),
            );
            let problem = "but the window names a key that types by what it types, with no Shift";
            return Err(format!(r#"gives "{text}", {problem}: write {instead}"#));
        }
        if key.first() == DEVTOOLS {
            return Err(format!(
                r#"gives "{text}", but WebView2 opens DevTools on Ctrl+Shift+I in a development build"#
            ));
        }
        if key.keeps_its_work() && binding.key != Some(key) {
            return Err(format!(
                r#"gives "{text}", but without Ctrl or Alt that key keeps its own work"#
            ));
        }
        Ok(key)
    }
}

/// The lines of `[keys]`, in the file's order, which says who keeps a key that two lines give (#550): those
/// Tsugiki can read, and the problems with the others, which leave their actions at their default keys. The
/// window reads the remaps as a map from each action's name to its key, `""` for none.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Remaps {
    remaps: Vec<Remap>,
    unread: Vec<(String, String)>,
}

impl Remaps {
    /// Reads the lines of `[keys]`, each an action's name and its value, with no value for one that isn't
    /// text.
    pub fn read(lines: Vec<(String, Option<String>)>) -> Self {
        let mut read = Self::default();
        for (name, value) in lines {
            match Remap::read(&name, value.as_deref()) {
                Ok(remap) => read.remaps.push(remap),
                Err(problem) => read.unread.push((name, problem)),
            }
        }
        read
    }

    /// The lines Tsugiki can't read, each by its name with what is wrong, in words for the status bar that
    /// follow the name.
    pub fn unread(&self) -> &[(String, String)] {
        &self.unread
    }
}

impl Serialize for Remaps {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.remaps.len()))?;
        for remap in &self.remaps {
            let key = remap.key.map_or_else(String::new, |key| key.to_string());
            map.serialize_entry(&remap.action, &key)?;
        }
        map.end()
    }
}

#[cfg(test)]
mod tests;
