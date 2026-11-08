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
mod tests {
    use super::*;

    fn strings(texts: &[&str]) -> Vec<String> {
        texts.iter().map(|text| (*text).to_owned()).collect()
    }

    #[test]
    fn a_scheme_counts_whatever_its_case() {
        let text = "TSUGIKI://pr/python/cpython/140124";
        assert_eq!(
            Request::parse(Build::Release, text),
            Request::Link(text.to_owned())
        );
    }

    #[test]
    fn a_link_in_the_other_builds_scheme_is_only_an_argument() {
        let dev = "tsugiki-dev://pr/python/cpython/140124";
        let release = "tsugiki://pr/python/cpython/140124";
        assert_eq!(
            Request::parse(Build::Release, dev),
            Request::Argument(dev.to_owned())
        );
        assert_eq!(
            Request::parse(Build::Development, release),
            Request::Argument(release.to_owned())
        );
    }

    #[test]
    fn anything_else_is_an_argument() {
        for text in ["python/cpython#140124", r"C:\Work\cpython", "tsugiki", ""] {
            assert_eq!(
                Request::parse(Build::Release, text),
                Request::Argument(text.to_owned())
            );
        }
    }

    #[test]
    fn the_program_name_isnt_a_request() {
        let argv = strings(&[
            r"C:\Programs\tsugiki:\tsg.exe",
            "tsugiki://a",
            "python/cpython",
        ]);
        assert_eq!(
            requests(Build::Release, &argv),
            vec![
                Request::Link("tsugiki://a".to_owned()),
                Request::Argument("python/cpython".to_owned()),
            ]
        );
    }

    #[test]
    fn a_program_started_with_nothing_asks_for_nothing() {
        assert_eq!(requests(Build::Release, &strings(&["tsg"])), vec![]);
        assert_eq!(requests(Build::Release, &[]), vec![]);
    }

    #[test]
    fn what_a_request_says_is_one_line_whatever_it_was_given() {
        // Any web page can open a link, so it mustn't be able to write a line of its own in the log.
        let forged = "tsugiki://a\n2026-11-03T00:00:00Z ERROR tsugiki_core: forged\u{1b}[31m";
        assert_eq!(
            Request::parse(Build::Release, forged).to_string(),
            r#"Asked to open the link "tsugiki://a\n2026-11-03T00:00:00Z ERROR tsugiki_core: forged\u{1b}[31m""#
        );
        assert_eq!(
            Request::parse(Build::Release, "a\nb").to_string(),
            r#"Asked to open the argument "a\nb""#
        );
    }

    #[test]
    fn this_programs_command_line_starts_with_its_name() {
        assert!(!command_line().is_empty());
    }

    #[test]
    fn opening_a_command_line_opens_each_request() {
        // Until Alerts arrive opening only logs, and the real-window check reads that line.
        open_command_line(Build::Release, &strings(&["tsg", "tsugiki://a", "b"]));
    }

    #[test]
    fn opening_one_link_needs_no_command_line() {
        open(Build::Release, "tsugiki://a");
    }
}
