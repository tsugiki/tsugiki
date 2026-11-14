//! The LSP values this client sends and reads, parsed where they arrive. LSP defines many more, and each
//! one here is one a caller uses.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use url::Url;

#[cfg(test)]
mod tests;

/// A place in a document: a 0-based line, and a column in UTF-16 code units, which is how LSP counts by
/// default. [`crate::utf16`] converts a column to and from a byte in the line's text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

/// From `start` up to, not including, `end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

/// A document's address. The files a server reads have `file:` URIs.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Uri(Url);

impl Uri {
    /// The `file:` URI of an absolute path, or `None` for a relative one.
    pub fn from_path(path: &Path) -> Option<Self> {
        Url::from_file_path(drive_upper(path)).ok().map(Self)
    }

    /// The URI in the one form this crate writes for each file, so two URIs of a file are equal and hash
    /// alike: servers on Windows can write the drive in either case, and its colon encoded, as
    /// `file:///c%3A/x`.
    fn normal(url: Url) -> Self {
        match url.to_file_path() {
            Ok(path) => Self::from_path(&path).unwrap_or(Self(url)),
            Err(()) => Self(url),
        }
    }

    /// The path a `file:` URI names, or `None` for another scheme. Servers can write a Windows drive with
    /// either case and its colon encoded, as `file:///c%3A/x`, and the path comes out the same.
    pub fn to_path(&self) -> Option<PathBuf> {
        self.0.to_file_path().ok()
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl TryFrom<String> for Uri {
    type Error = url::ParseError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Url::parse(&text).map(Self::normal)
    }
}

/// `path` with its first letter upper case: a Windows drive's. An absolute path elsewhere starts with `/`,
/// which stays as it is.
fn drive_upper(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    let mut characters = text.chars();
    let first = characters.next().map(|first| first.to_ascii_uppercase());
    PathBuf::from(first.into_iter().chain(characters).collect::<String>())
}

impl From<Uri> for String {
    fn from(uri: Uri) -> Self {
        uri.0.into()
    }
}

impl fmt::Display for Uri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A range in a document.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Location {
    pub uri: Uri,
    pub range: Range,
}

/// A definition as a server that supports links gives it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationLink {
    /// The name the lookup was asked on, as the server saw it, when it says.
    pub origin_selection_range: Option<Range>,
    pub target_uri: Uri,
    /// All of the definition, such as a whole function with its body.
    pub target_range: Range,
    /// The part of `target_range` to show, such as the function's name.
    pub target_selection_range: Range,
}

/// The places a name is defined: none, one or several. A server answers with locations or with links, and
/// LSP gives it no way to mix them.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(from = "DefinitionReply")]
pub enum Definitions {
    Locations(Vec<Location>),
    Links(Vec<LocationLink>),
}

impl Definitions {
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Locations(locations) => locations.is_empty(),
            Self::Links(links) => links.is_empty(),
        }
    }

    /// Each place as a location: a link's is the part of its target to show, such as a function's name.
    pub fn into_locations(self) -> Vec<Location> {
        match self {
            Self::Locations(locations) => locations,
            Self::Links(links) => (links.into_iter())
                .map(|link| Location {
                    uri: link.target_uri,
                    range: link.target_selection_range,
                })
                .collect(),
        }
    }
}

/// Each shape LSP lets a server answer `textDocument/definition` with.
#[derive(Deserialize)]
#[serde(untagged)]
enum DefinitionReply {
    None(()),
    One(Location),
    Many(Vec<Location>),
    Links(Vec<LocationLink>),
}

impl From<DefinitionReply> for Definitions {
    fn from(reply: DefinitionReply) -> Self {
        match reply {
            DefinitionReply::None(()) => Self::Locations(Vec::new()),
            DefinitionReply::One(location) => Self::Locations(vec![location]),
            DefinitionReply::Many(locations) => Self::Locations(locations),
            DefinitionReply::Links(links) => Self::Links(links),
        }
    }
}

/// The server's name and version, from its answer to `initialize`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: Option<String>,
}

/// The parts of the answer to `initialize` a caller reads.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InitializeResult {
    pub(crate) server_info: Option<ServerInfo>,
}

/// A language, by its LSP identifier, which a document is opened with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    C,
    Python,
    Rust,
}

impl Language {
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::C => "c",
            Self::Python => "python",
            Self::Rust => "rust",
        }
    }
}

/// The problems a server found in a document, and the document's version it looked at, when it says.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Diagnostics {
    pub uri: Uri,
    pub version: Option<i32>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Diagnostic {
    pub range: Range,
    pub severity: Option<Severity>,
    /// The tool that reported it, such as `clang`.
    pub source: Option<String>,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "u8")]
pub enum Severity {
    Error,
    Warning,
    Information,
    Hint,
}

impl TryFrom<u8> for Severity {
    type Error = String;

    fn try_from(number: u8) -> Result<Self, String> {
        match number {
            1 => Ok(Self::Error),
            2 => Ok(Self::Warning),
            3 => Ok(Self::Information),
            4 => Ok(Self::Hint),
            other => Err(format!("{other} isn't a diagnostic's severity")),
        }
    }
}

/// How rust-analyzer stands, from its `experimental/serverStatus`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Status {
    /// Whether it has loaded the workspace and has no work left that could change an answer.
    pub quiescent: bool,
    /// What it says is wrong, if anything is, such as a workspace cargo couldn't read.
    pub message: Option<String>,
}

/// A server's progress on a piece of work, such as clangd's index.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Progress {
    pub token: ProgressToken,
    pub value: WorkDone,
}

/// The piece of work a [`Progress`] is about, by a token the server picks.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum ProgressToken {
    Number(i64),
    Text(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum WorkDone {
    Begin {
        title: String,
        message: Option<String>,
        percentage: Option<u32>,
    },
    Report {
        message: Option<String>,
        percentage: Option<u32>,
    },
    End {
        message: Option<String>,
    },
}

/// The settings sections a server asks for with `workspace/configuration`.
#[derive(Deserialize)]
pub(crate) struct ConfigurationParams {
    pub(crate) items: Vec<ConfigurationItem>,
}

#[derive(Deserialize)]
pub(crate) struct ConfigurationItem {
    /// A dotted path into the settings, such as `ty.environment`, or `None` for all of them.
    pub(crate) section: Option<String>,
}
