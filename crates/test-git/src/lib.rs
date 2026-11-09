//! Git repositories for the tests of `tsugiki-core` and `tsugiki-app`, built with the real git (ADR 0009).
//! Core's `testing` modules are private to core, so the helpers both crates' tests share live here, and
//! a change to how a test runs git is made once.

#![expect(
    clippy::disallowed_types,
    reason = "tests build their repositories with the real git (ADR 0009)"
)]
#![expect(clippy::unwrap_used, reason = "a failed setup step ends the test")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The author of each commit a test makes, so a commit needs no git config on the machine.
const AUTHOR: [&str; 4] = [
    "-c",
    "user.name=Contributor",
    "-c",
    "user.email=contributor@example.com",
];

/// What points git at another repository than the one in the folder it runs in. A hook or an alias that
/// runs the tests can leave these set, and then `reset --hard` would act on the developer's checkout.
const ELSEWHERE: [&str; 3] = ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"];

/// Runs git in `dir` and gives what it printed. A git that fails ends the test with what it said.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    for name in ELSEWHERE {
        let _ = command.env_remove(name);
    }
    let output = command
        .args(AUTHOR)
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// Makes a bare repository for `name`, an `owner/name`, under `remotes` where GitHub keeps it:
/// `owner/name.git`. It has one commit on `main` and the tag `v3.15.0a0`. The working copy it came from
/// stays at `work/owner/name`, for [`pull_request`] to commit in.
pub fn remote(remotes: &Path, name: &str) {
    let work = work(remotes, name);
    fs::create_dir_all(&work).unwrap();
    let _ = git(&work, &["init", "-q", "-b", "main"]);
    fs::write(
        work.join("README.rst"),
        "This is Python version 3.15.0 alpha 0\n",
    )
    .unwrap();
    let _ = git(&work, &["add", "README.rst"]);
    let _ = git(&work, &["commit", "-qm", "One"]);
    let _ = git(&work, &["tag", "v3.15.0a0"]);
    let bare = bare(remotes, name);
    let _ = git(
        remotes,
        &[
            "clone",
            "-q",
            "--bare",
            &work.to_string_lossy(),
            &bare.to_string_lossy(),
        ],
    );
}

/// Puts a commit of PR `number`'s own on the [`remote`] for `name`, under `refs/pull/N/head` as GitHub
/// keeps it, on no branch. Gives the commit.
pub fn pull_request(remotes: &Path, name: &str, number: u32) -> String {
    let work = work(remotes, name);
    fs::write(work.join("NEWS"), format!("PR {number}\n")).unwrap();
    let _ = git(&work, &["add", "NEWS"]);
    let _ = git(&work, &["commit", "-qm", &format!("PR {number}")]);
    let commit = git(&work, &["rev-parse", "HEAD"]).trim().to_owned();
    let _ = git(
        &work,
        &[
            "push",
            "-q",
            &bare(remotes, name).to_string_lossy(),
            &format!("HEAD:refs/pull/{number}/head"),
        ],
    );
    let _ = git(&work, &["reset", "-q", "--hard", "HEAD~1"]);
    commit
}

/// Where `remotes` are, as a `file://` URL that ends in a slash. git reports progress over it as it does
/// from GitHub.
pub fn url(remotes: &Path) -> String {
    let path = remotes.to_string_lossy().replace('\\', "/");
    let slash = if path.starts_with('/') { "" } else { "/" };
    format!("file://{slash}{path}/")
}

fn work(remotes: &Path, name: &str) -> PathBuf {
    remotes.join("work").join(name)
}

fn bare(remotes: &Path, name: &str) -> PathBuf {
    remotes.join(format!("{name}.git"))
}
