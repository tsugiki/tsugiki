//! Tsugiki's keys: each action the user can bind, its title, its default key and where it works (#548). The
//! window binds actions, not keys, and finds a key's action in this table, which `npm run bindings` writes
//! to `ui/src/lib/data/keymap.ts`. The Settings Keyboard page remaps by it, and `[keys]` in settings.toml
//! names each action as serde names it here.

use std::fmt;

use serde::{Serialize, Serializer};

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
}

/// One press: a key and the modifiers held with it. It reads as the window's `keyName` names a press,
/// "ctrl+shift+p", with the key in lower case and the modifiers in the order Ctrl, Alt, Shift.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Press {
    modifiers: Modifiers,
    /// What the key types, or its name: "p", "?", "f7", "pagedown", "arrowleft".
    key: &'static str,
}

impl fmt::Display for Press {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let held = match self.modifiers {
            Modifiers::Plain => "",
            Modifiers::Shift => "shift+",
            Modifiers::Ctrl => "ctrl+",
            Modifiers::Alt => "alt+",
            Modifiers::CtrlShift => "ctrl+shift+",
            Modifiers::CtrlAlt => "ctrl+alt+",
        };
        write!(f, "{held}{}", self.key)
    }
}

/// An action's key: one press, or a chord of two, such as `Ctrl+K M`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keys {
    One(Press),
    Chord(Press, Press),
}

impl Keys {
    /// Whether the two can't both stand: the same key, or one press that starts the other's chord, which
    /// would then never wait for its second key.
    fn clashes(self, other: Self) -> bool {
        match (self, other) {
            (Self::One(press), Self::Chord(first, _))
            | (Self::Chord(first, _), Self::One(press)) => press == first,
            (Self::One(_), Self::One(_)) | (Self::Chord(..), Self::Chord(..)) => self == other,
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
}

/// An action's default key, written in `table!` as its modifiers and key, two of those for a chord, or
/// nothing for no key. A macro and not a `const fn`, which would run only as the code compiles, where the
/// coverage gate can't see it.
macro_rules! keys {
    () => {
        None
    };
    ($held:ident $key:literal) => {
        Some(Keys::One(Press {
            modifiers: Modifiers::$held,
            key: $key,
        }))
    };
    ($held:ident $key:literal, $then_held:ident $then:literal) => {
        Some(Keys::Chord(
            Press {
                modifiers: Modifiers::$held,
                key: $key,
            },
            Press {
                modifiers: Modifiers::$then_held,
                key: $then,
            },
        ))
    };
}

/// Writes the `Action` enum and `TABLE` from one list, so an action can't miss its row or have two.
macro_rules! table {
    ($($action:ident: $title:literal, ($($key:tt)*), $context:ident;)*) => {
        /// An action the user can bind a key to, named in `[keys]` as serde names it: `archive`,
        /// `local_mark`.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
        #[serde(rename_all = "snake_case")]
        pub enum Action {
            $($action,)*
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
    CommandPalette: "Command Palette", (CtrlShift "p"), Anywhere;
    QuickOpen: "Quick Open", (Ctrl "p"), Anywhere;
    GoToDefinition: "Go to Definition", (Plain "f12"), Diff;
    PeekDefinition: "Peek Definition", (Alt "f12"), Diff;
    NextChange: "Next Change", (Plain "f7"), Diff;
    PreviousChange: "Previous Change", (Shift "f7"), Diff;
    NextThread: "Next Comment Thread", (Plain "f8"), Diff;
    PreviousThread: "Previous Comment Thread", (Shift "f8"), Diff;
    FindInDiff: "Find in Diff", (Ctrl "f"), Diff;
    ToggleSidebar: "Hide or Show the Sidebar", (Ctrl "b"), Anywhere;
    ToggleIssueList: "Hide or Show the Issue List", (CtrlAlt "b"), Anywhere;
    Unfold: "Unfold an Issue's PRs", (Plain "arrowright"), IssueList;
    Fold: "Fold an Issue's PRs", (Plain "arrowleft"), IssueList;
    ToggleDetailOnly: "Show Only the Detail Pane, or the Panes", (Ctrl "k", Plain "z"), Anywhere;
    ToggleDiffView: "Unified or Side by Side", (CtrlAlt "u"), Diff;
    ToggleFilesMode: "All Files or One File at a Time", (Ctrl "k", Plain "o"), Diff;
    UnfoldUnchanged: "Unfold Unchanged Files", (Ctrl "k", Plain "j"), Diff;
    NextTab: "Next Tab", (Ctrl "pagedown"), Anywhere;
    PreviousTab: "Previous Tab", (Ctrl "pageup"), Anywhere;
    NextFile: "Next File", (Alt "pagedown"), Diff;
    PreviousFile: "Previous File", (Alt "pageup"), Diff;
    SendComment: "Send Comment", (Ctrl "enter"), Composer;
    OpenInEditor: "Open in Editor", (CtrlShift "e"), Diff;
    LocalMark: "Local Mark", (Ctrl "k", Plain "m"), Anywhere;
    Pin: "Pin or Unpin", (Ctrl "k", Plain "p"), Anywhere;
    PickTheme: "Pick a Theme", (Ctrl "k", Plain "t"), Anywhere;
    ReloadThemes: "Reload Themes", (Ctrl "k", Plain "r"), Anywhere;
    GoBack: "Go Back", (Alt "arrowleft"), Anywhere;
    GoForward: "Go Forward", (Alt "arrowright"), Anywhere;
    OpenOnGithub: "Open on GitHub", (CtrlShift "g"), Anywhere;
    SwitchRepository: "Switch Repository", (Ctrl "r"), Anywhere;
    ResetLayout: "Reset Layout", (), Anywhere;
    Archive: "Archive", (Plain "e"), IssueList;
    Mute: "Mute", (Plain "m"), IssueList;
    MarkSeen: "Mark Seen", (Plain "q"), Anywhere;
    MarkUnseen: "Mark Unseen", (Shift "q"), Anywhere;
    OpenSettings: "Open Settings", (Ctrl ","), Anywhere;
    ZoomIn: "Zoom In", (Ctrl "="), Anywhere;
    ZoomOut: "Zoom Out", (Ctrl "-"), Anywhere;
    ZoomReset: "Reset Zoom", (Ctrl "0"), Anywhere;
    ToggleHud: "Show or Hide the Performance HUD", (CtrlShift "h"), Anywhere;
    OpenDevtools: "Open DevTools", (CtrlShift "i"), Anywhere;
    ShowKeys: "List the View's Keys", (Plain "?"), Anywhere;
}

#[cfg(test)]
mod tests;
