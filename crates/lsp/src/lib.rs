//! A client that talks to one language server over its standard streams (ADR 0020).
//!
//! It knows nothing of PRs, Mirrors or the database: the caller says which server to start, which
//! documents to open and what to ask. One thread owns each server, and [`Server`]'s methods send it
//! messages. The README says what the crate does and doesn't do.

#![forbid(unsafe_code)]

mod client;
mod framing;
mod process;
mod rpc;
#[cfg(test)]
mod testing;
mod types;
pub mod utf16;

pub use client::{
    Canceller, Document, Event, Exit, Init, Pending, Process, RequestError, Server, Starting,
    Streams,
};
pub use process::Launch;
pub use types::{
    Definitions, Diagnostic, Diagnostics, Language, Location, LocationLink, Position, Progress,
    ProgressToken, Range, ServerInfo, Severity, Uri, WorkDone,
};
