//! When Tsugiki's process ends, however it ends, its language servers and the processes they started end
//! with it (#682). A parent process stands in for Tsugiki: it starts a server, which starts a worker, and
//! the test kills the parent. Each of the three is this test binary run again in another role.
//!
//! Windows ends them through a job object. Off Windows a crash only closes the server's input, which waits
//! for the macOS checks before 1.0, so this test runs on Windows alone.

#![cfg(windows)]
#![expect(
    clippy::disallowed_types,
    reason = "the test starts the parent, and the server starts the worker"
)]
#![expect(
    clippy::unwrap_used,
    reason = "a test fails by panicking, and each role runs outside the #[test] function's body"
)]
#![expect(
    clippy::zombie_processes,
    reason = "the server never waits for its worker: the test kills both"
)]

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::time::{Duration, Instant};
use std::{env, thread};

use tsugiki_lsp::{Launch, Server};

const TEST: &str = "a_killed_parent_leaves_no_server_behind";
/// The role a run of this binary plays: none for the test itself.
const ROLE: &str = "TSUGIKI_LSP_CRASH_ROLE";
/// The folder where each role locks a file while it runs.
const FOLDER: &str = "TSUGIKI_LSP_CRASH_FOLDER";
/// Longer than any role should take to start, or a process to end once killed.
const PATIENCE: Duration = Duration::from_secs(30);
/// The time a role runs if nothing ends it.
const LIFETIME: Duration = Duration::from_secs(120);

#[test]
fn a_killed_parent_leaves_no_server_behind() {
    match env::var(ROLE).as_deref() {
        Ok("parent") => parent(),
        Ok("server") => server(),
        Ok("worker") => hold("worker"),
        _ => {}
    }
    let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.temp")
        .join(format!("lsp-crash-{}", process::id()));
    if folder.exists() {
        fs::remove_dir_all(&folder).unwrap();
    }
    fs::create_dir_all(&folder).unwrap();
    let mut parent = Command::new(env::current_exe().unwrap())
        .args(rerun())
        .env(ROLE, "parent")
        .env(FOLDER, &folder)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    for role in ["server", "worker"] {
        let ready = folder.join(format!("{role}.ready"));
        assert!(
            within(PATIENCE, || ready.exists()),
            "the {role} didn't start"
        );
    }

    parent.kill().unwrap();
    let _killed = parent.wait().unwrap();
    for role in ["server", "worker"] {
        let lock = File::open(folder.join(format!("{role}.lock"))).unwrap();
        assert!(
            within(PATIENCE, || lock.try_lock().is_ok()),
            "the {role} outlived its parent"
        );
    }
}

/// The arguments that run this test alone, in the role the environment names.
fn rerun() -> [&'static str; 3] {
    ["--exact", TEST, "--nocapture"]
}

fn folder() -> PathBuf {
    PathBuf::from(env::var_os(FOLDER).unwrap())
}

/// Tsugiki: starts a server, which it never asks to stop, and waits to be killed.
fn parent() -> ! {
    let launch = Launch {
        program: env::current_exe().unwrap(),
        args: rerun().map(Into::into).to_vec(),
        folder: folder(),
        env: vec![
            (ROLE.into(), "server".into()),
            (FOLDER.into(), folder().into()),
        ],
    };
    let (_starting, _events) = Server::start(&launch).unwrap();
    thread::sleep(LIFETIME);
    process::exit(0);
}

/// A server that never reads what the client writes, so it doesn't notice its input close, and that
/// starts a worker.
fn server() -> ! {
    let _worker = Command::new(env::current_exe().unwrap())
        .args(rerun())
        .env(ROLE, "worker")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    hold("server")
}

/// Locks a file named for `role` until the process ends, then says it is ready.
fn hold(role: &str) -> ! {
    let lock = File::create(folder().join(format!("{role}.lock"))).unwrap();
    lock.lock().unwrap();
    fs::write(folder().join(format!("{role}.ready")), "").unwrap();
    thread::sleep(LIFETIME);
    process::exit(0);
}

/// Whether `check` holds within `patience`.
fn within(patience: Duration, check: impl Fn() -> bool) -> bool {
    let until = Instant::now() + patience;
    while Instant::now() < until {
        if check() {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    check()
}
