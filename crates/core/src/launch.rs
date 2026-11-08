//! The one handler for what `tsg` is asked to open (ADR 0007, ADR 0016). A `tsugiki://` link arrives on the
//! command line on Windows and from the OS on macOS, and a second `tsg` passes its command line to the first.
//!
//! Any web page can open a link and any program can start `tsg`, so nothing here can be trusted. Until Alerts
//! arrive, opening only logs.

use std::fmt;

use crate::build::Build;

/// One thing `tsg` was asked to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// A link in this build's scheme.
    Link(String),
    /// Anything else, including a link in another build's scheme.
    Argument(String),
}

impl Request {
    /// Sorts `text` by its scheme, whose case doesn't count (RFC 3986), since `tsg` can get a typed link.
    pub fn parse(build: Build, text: &str) -> Self {
        let is_link = text
            .split_once(':')
            .is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case(build.scheme()));
        if is_link {
            Self::Link(text.to_owned())
        } else {
            Self::Argument(text.to_owned())
        }
    }
}

/// `{:?}` escapes line breaks and control characters, so a request can't write a log line of its own.
impl fmt::Display for Request {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Link(link) => write!(formatter, "Asked to open the link {link:?}"),
            Self::Argument(argument) => {
                write!(formatter, "Asked to open the argument {argument:?}")
            }
        }
    }
}

/// This program's command line. An argument that isn't valid Unicode gets U+FFFD in its bad parts, which is
/// also what a second `tsg` sends, and `std::env::args` would panic on it.
pub fn command_line() -> Vec<String> {
    std::env::args_os()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect()
}

/// What a command line asks for. `argv` starts with the program's name, as `command_line` does.
pub fn requests(build: Build, argv: &[String]) -> Vec<Request> {
    argv.iter()
        .skip(1)
        .map(|argument| Request::parse(build, argument))
        .collect()
}

/// Opens what a command line asks for.
pub fn open_command_line(build: Build, argv: &[String]) {
    for request in requests(build, argv) {
        log::info!("{request}");
    }
}

/// Opens one link or argument that came without a command line, as a link does on macOS.
pub fn open(build: Build, text: &str) {
    log::info!("{}", Request::parse(build, text));
}

#[cfg(test)]
mod tests;
