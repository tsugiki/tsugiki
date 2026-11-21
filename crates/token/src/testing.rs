//! What the tests of this crate and the crates that use it give in place of gh and the environment, and the
//! only way outside this crate to build a [`TokenFrom`] other than [`TokenFrom::LIVE`]. Other crates reach it
//! through the `testing` feature, which only their `[dev-dependencies]` turn on.

use std::io;

use crate::{GhOutput, ReadEnv, RunGh, TokenFrom};

/// What a gh that worked and printed `stdout` gives.
pub fn gh_prints(stdout: &str) -> io::Result<GhOutput> {
    Ok(GhOutput {
        success: true,
        stdout: stdout.to_owned(),
    })
}

/// An environment with no `GH_TOKEN`.
pub(crate) fn nothing_set() -> Option<String> {
    None
}

/// A machine with no `GH_TOKEN`, so the token comes from `gh`.
pub const fn gh_alone(gh: RunGh) -> TokenFrom {
    TokenFrom {
        env: nothing_set,
        gh,
    }
}

/// What running a `gh` that isn't installed gives.
fn gh_not_found(_: &[&str]) -> io::Result<GhOutput> {
    Err(io::Error::from(io::ErrorKind::NotFound))
}

/// A machine with no `GH_TOKEN` and no `gh`.
pub const NO_GH: TokenFrom = gh_alone(gh_not_found);

/// A machine whose `GH_TOKEN` is what `env` gives, and with no `gh`.
pub const fn gh_token(env: ReadEnv) -> TokenFrom {
    TokenFrom {
        env,
        gh: gh_not_found,
    }
}
