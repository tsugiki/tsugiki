//! What the tests of this crate and the crates that use it give in place of gh. Other crates reach it
//! through the `testing` feature, which only their `[dev-dependencies]` turn on.

use std::io;

use crate::GhOutput;

/// What a gh that worked and printed `stdout` gives.
pub fn gh_prints(stdout: &str) -> io::Result<GhOutput> {
    Ok(GhOutput {
        success: true,
        stdout: stdout.to_owned(),
    })
}
