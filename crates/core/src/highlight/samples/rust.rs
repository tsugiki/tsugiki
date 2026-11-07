//! Reads a diff.

use std::fmt;

/// A line of a diff.
#[derive(Debug, Clone)]
pub enum Row<'a> {
    Added { new: u32, text: &'a str },
    Removed(u32),
}

impl fmt::Display for Row<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Added { new, text } => write!(formatter, "+{new} {text}"),
            Self::Removed(old) => write!(formatter, "-{old}"),
        }
    }
}

const LIMIT: usize = 512;
