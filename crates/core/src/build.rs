//! What keeps a development build apart from an installed Tsugiki, so the two can run side by side without
//! taking each other's links or sharing a database (ADR 0016).

/// Which kind of program this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Build {
    Release,
    Development,
}

impl Build {
    /// The kind this program is. `cfg!(debug_assertions)` is also what gives a development build its console.
    pub const CURRENT: Self = if cfg!(debug_assertions) {
        Self::Development
    } else {
        Self::Release
    };

    /// The bundle identifier. The single-instance lock and the webview's data are named after it.
    pub const fn identifier(self) -> &'static str {
        match self {
            Self::Release => "io.github.tsugiki",
            Self::Development => "io.github.tsugiki.dev",
        }
    }

    /// The scheme of the links this program answers.
    pub const fn scheme(self) -> &'static str {
        match self {
            Self::Release => "tsugiki",
            Self::Development => "tsugiki-dev",
        }
    }

    /// The name of the folders that hold the database and the settings, when this program isn't portable.
    pub(crate) const fn folder_name(self) -> &'static str {
        match self {
            Self::Release => "tsugiki",
            Self::Development => "tsugiki-dev",
        }
    }
}

#[cfg(test)]
mod tests;
