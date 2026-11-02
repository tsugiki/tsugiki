//! Record mode (ADR 0009): `npm run record` asks real GitHub the client's read-only queries, with gh's
//! token, and writes the scrubbed answers to `tests/fixtures/github`. Read them before you commit them.

use std::path::Path;

use tsugiki_core::github::GitHub;
use tsugiki_core::github::live::{Https, run_gh};
use tsugiki_core::github::record::Recorder;
use tsugiki_core::token::Token;

fn main() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/github");
    let token = Token::from_gh(run_gh).expect("read gh's token");
    let recorder = Recorder::new(Box::new(Https::new()), fixtures.clone());
    let github = GitHub::new(Box::new(recorder), token);
    github.viewer().expect("ask GitHub who the user is");
    println!("Recorded to {}", fixtures.display());
}
