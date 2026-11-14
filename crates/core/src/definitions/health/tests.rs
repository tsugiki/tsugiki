use super::*;

fn printed(text: &str) -> io::Result<String> {
    Ok(text.to_owned())
}

#[test]
fn each_server_s_version_is_its_first_word_that_starts_with_a_digit() {
    assert_eq!(
        version_in("ty 0.0.103 (f9672a390 2026-11-13)"),
        Some("0.0.103".to_owned())
    );
    let clangd = "clangd version 23.1.0 (https://github.com/llvm/llvm-project ea7d852a70e8)";
    assert_eq!(version_in(clangd), Some("23.1.0".to_owned()));
    assert_eq!(
        version_in("rust-analyzer 1.99.1 (58cf8f9c 2026-10-13)"),
        Some("1.99.1".to_owned())
    );
    assert_eq!(version_in("1.99.1, nightly"), Some("1.99.1".to_owned()));
    assert_eq!(version_in("clangd from a build of my own"), None);
}

#[test]
fn version_has_to_name_the_server_on_its_first_line() {
    let clangd = "\nclangd version 23.1.0 (https://github.com/llvm/llvm-project)\r\nFeatures: windows+grpc\r\n";
    assert_eq!(
        read_check(ServerName::Clangd, printed(clangd)),
        Ok(Some("23.1.0".to_owned()))
    );
    assert_eq!(
        read_check(ServerName::Ty, printed("Ty built by hand\n")),
        Ok(None)
    );
    assert_eq!(
        read_check(ServerName::Ty, printed("pretty 1.0\n")),
        Err(ServerProblem::NotThisServer {
            first_line: "pretty 1.0".to_owned()
        })
    );
    assert_eq!(
        read_check(ServerName::RustAnalyzer, printed("")),
        Err(ServerProblem::NotThisServer {
            first_line: String::new()
        })
    );
    let missing = io::Error::new(io::ErrorKind::NotFound, "not there");
    assert_eq!(
        read_check(ServerName::Clangd, Err(missing)),
        Err(ServerProblem::DidNotStart {
            cause: "not there".to_owned()
        })
    );
}

#[test]
fn a_version_we_didn_t_test_says_so_and_a_problem_outranks_whether_the_server_runs() {
    let health = |version: &str, last: Last| Health {
        version: Some(version.to_owned()),
        last,
    };
    let untested = health("0.0.110", Last::Nothing).row(ServerName::Ty, Presence::Running);
    let stopped = ServerProblem::Stopped {
        code: Some(1),
        stderr_tail: String::new(),
    };
    let troubled = health("23.1.0", Last::Problem(stopped.clone()));
    let idle = health("1.99.1", Last::Idle);

    let found = FoundVersion {
        version: "0.0.110".to_owned(),
        tested: false,
    };
    assert_eq!(untested.version, Some(found));
    assert_eq!(untested.status, ServerStatus::Running);
    assert_eq!(untested.tested, ["0.0.103"]);
    assert_eq!(
        troubled.row(ServerName::Clangd, Presence::Running).status,
        ServerStatus::Problem { problem: stopped }
    );
    let not_set_up = troubled.row(ServerName::Clangd, Presence::NotSetUp);
    assert_eq!(not_set_up.status, ServerStatus::NotSetUp);
    let row = idle.row(ServerName::RustAnalyzer, Presence::Stopped);
    assert_eq!(row.status, ServerStatus::Idle);
    assert_eq!(row.version.map(|found| found.tested), Some(true));
    let mut started = idle;
    started.started(None);
    assert_eq!(
        started
            .row(ServerName::RustAnalyzer, Presence::Stopped)
            .status,
        ServerStatus::Ready
    );
    assert_eq!(started.version.as_deref(), Some("1.99.1"));
}

#[test]
fn a_launcher_that_leaves_a_process_holding_its_output_gives_its_version_all_the_same() {
    let folder = crate::testing::temp_path("launcher");
    std::fs::create_dir_all(&folder).unwrap();
    let launcher = launcher_in(&folder);

    let started = std::time::Instant::now();
    let checked = check(ServerName::Ty, &launcher);
    let took = started.elapsed();
    std::fs::remove_dir_all(&folder).unwrap();

    assert_eq!(checked, Ok(Some("0.0.1".to_owned())));
    assert!(took < CHECK_PATIENCE, "{took:?}");
}

/// A program in `folder` that prints ty's version and leaves a process behind that holds its output for
/// half a minute.
#[cfg(windows)]
fn launcher_in(folder: &Path) -> std::path::PathBuf {
    let path = folder.join("ty.cmd");
    let script = "@echo off\r\necho ty 0.0.1\r\nstart /b cmd /c \"ping -n 30 127.0.0.1 >nul\"\r\n";
    std::fs::write(&path, script).unwrap();
    path
}

/// A program in `folder` that prints ty's version and leaves a process behind that holds its output for
/// half a minute.
#[cfg(not(windows))]
fn launcher_in(folder: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = folder.join("ty");
    std::fs::write(&path, "#!/bin/sh\necho ty 0.0.1\nsleep 30 &\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[test]
fn version_names_the_server_on_a_real_program() {
    let ty = super::super::testing::ty();

    assert_eq!(check(ServerName::Ty, &ty), Ok(Some("0.0.103".to_owned())));
}
