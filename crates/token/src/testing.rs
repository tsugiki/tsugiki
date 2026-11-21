//! What the tests of this crate and the crates that use it give in place of gh and the environment. Other
//! crates reach it through the `testing` feature, which only their `[dev-dependencies]` turn on.

use std::io;

use crate::{GhOutput, RunGh, TokenFrom};

/// What a gh that worked and printed `stdout` gives.
pub fn gh_prints(stdout: &str) -> io::Result<GhOutput> {
    Ok(GhOutput {
        success: true,
        stdout: stdout.to_owned(),
    })
}

/// An environment that sets no variable.
pub fn nothing_set(_: &str) -> Option<String> {
    None
}

/// A machine with no `GH_TOKEN`, so the token comes from `gh`.
pub fn gh_alone(gh: RunGh) -> TokenFrom {
    TokenFrom {
        env: nothing_set,
        gh,
    }
}
