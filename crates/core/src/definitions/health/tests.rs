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
    let health = |version: &str| Health {
        version: Some(version.to_owned()),
        check: Check::Clear,
        flaw: None,
    };
    let untested = health("0.0.110").row(ServerName::Ty, Presence::Running, &Last::Nothing);
    let stopped = ServerProblem::Stopped {
        code: Some(1),
        stderr_tail: String::new(),
    };
    let troubled = Last::Problem(stopped.clone());

    let found = FoundVersion {
        version: "0.0.110".to_owned(),
        tested: false,
    };
    assert_eq!(untested.version, Some(found));
    assert_eq!(untested.status, ServerStatus::Running);
    assert_eq!(untested.tested, ["0.0.103"]);
    let clangd = health("23.1.0");
    assert_eq!(
        clangd
            .row(ServerName::Clangd, Presence::Running, &troubled)
            .status,
        ServerStatus::Problem { problem: stopped }
    );
    let not_set_up = clangd.row(ServerName::Clangd, Presence::NotSetUp, &troubled);
    assert_eq!(not_set_up.status, ServerStatus::NotSetUp);
    let row = health("1.99.1").row(ServerName::RustAnalyzer, Presence::Stopped, &Last::Idle);
    assert_eq!(row.status, ServerStatus::Idle);
    assert_eq!(row.version.map(|found| found.tested), Some(true));
    let ready = health("1.99.1").row(ServerName::RustAnalyzer, Presence::Stopped, &Last::Nothing);
    assert_eq!(ready.status, ServerStatus::Ready);
}

#[test]
fn a_failed_check_shows_until_a_server_starts_and_a_check_that_answers_after_a_start_counts_for_nothing()
 {
    let wrong = ServerProblem::NotThisServer {
        first_line: "pretty 1.0".to_owned(),
    };
    let status = |health: &Health| {
        let row = health.row(ServerName::Ty, Presence::Stopped, &Last::Nothing);
        (row.status, row.version.map(|found| found.version))
    };
    let mut failed = Health::checking();
    failed.checked(Err(wrong.clone()));
    let mut passed = Health::checking();
    passed.checked(Ok(Found {
        version: Some("0.0.103".to_owned()),
        flaw: None,
    }));
    let mut superseded = Health::checking();
    superseded.started(Some("0.0.104".to_owned()), None);
    superseded.checked(Err(wrong.clone()));

    assert_eq!(status(&Health::checking()), (ServerStatus::Ready, None));
    assert_eq!(
        status(&failed),
        (ServerStatus::Problem { problem: wrong }, None)
    );
    failed.started(None, None);
    assert_eq!(status(&failed), (ServerStatus::Ready, None));
    assert_eq!(
        status(&passed),
        (ServerStatus::Ready, Some("0.0.103".to_owned()))
    );
    assert_eq!(
        status(&superseded),
        (ServerStatus::Ready, Some("0.0.104".to_owned()))
    );
}

#[test]
fn a_launcher_that_leaves_a_process_holding_its_output_gives_its_version_all_the_same() {
    let folder = crate::testing::temp_path("launcher");
    std::fs::create_dir_all(&folder).unwrap();
    let launcher = launcher_in(&folder);

    let started = std::time::Instant::now();
    let checked = check(ServerName::Ty, &launcher);
    let took = started.elapsed();

    let found = Found {
        version: Some("0.0.1".to_owned()),
        flaw: None,
    };
    assert_eq!(checked, Ok(found));
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
fn a_problem_of_a_server_that_runs_gives_way_only_to_one_that_explains_more() {
    let program = Health::default();
    let mut last = Last::Problem(ServerProblem::DidNotStart {
        cause: "gone".to_owned(),
    });
    let missing = |header: &str| ServerProblem::SystemHeadersMissing {
        header: header.to_owned(),
    };

    let kept_from_answering = program.misleads(&last).cloned();
    let after_a_failed_start = last.troubled(ServerProblem::NotIndexing);
    let headers = last.troubled(missing("stdlib.h"));
    let less = last.troubled(ServerProblem::NotIndexing);
    let as_much = last.troubled(missing("windows.h"));
    let built_ins = last.troubled(ServerProblem::BuiltInHeadersMissing);

    assert_eq!(kept_from_answering, None);
    assert!(after_a_failed_start && headers && built_ins);
    assert!(!less && !as_much);
    assert_eq!(
        program.misleads(&last),
        Some(&ServerProblem::BuiltInHeadersMissing)
    );
    let mut idle = Last::Idle;
    assert_eq!(program.misleads(&idle), None);
    assert!(idle.troubled(ServerProblem::NotIndexing));
}

#[test]
fn what_a_program_lacks_shows_and_explains_until_a_start_finds_it_no_longer_does() {
    let mut lacking = Health::checking();
    lacking.checked(Ok(Found {
        version: Some("23.1.0".to_owned()),
        flaw: Some(ServerProblem::BuiltInHeadersMissing),
    }));
    let status = |health: &Health| {
        let row = health.row(ServerName::Clangd, Presence::Running, &Last::Nothing);
        row.status
    };
    let mut not_indexing = Last::Problem(ServerProblem::NotIndexing);

    let shown = status(&lacking);
    let explains = lacking.misleads(&not_indexing).cloned();
    not_indexing.indexed();
    lacking.started(None, None);

    let problem = ServerProblem::BuiltInHeadersMissing;
    assert_eq!(
        shown,
        ServerStatus::Problem {
            problem: problem.clone()
        }
    );
    assert_eq!(explains, Some(problem));
    assert!(matches!(not_indexing, Last::Nothing));
    assert_eq!(status(&lacking), ServerStatus::Running);
}

#[test]
fn a_clangd_whose_version_names_it_is_checked_for_its_built_in_headers_too() {
    let clangd = super::super::testing::clangd();

    let checked = check(ServerName::Clangd, &clangd);

    let found = Found {
        version: Some("23.1.0".to_owned()),
        flaw: None,
    };
    assert_eq!(checked, Ok(found));
}

#[test]
fn version_names_the_server_on_a_real_program() {
    let ty = super::super::testing::ty();

    let found = Found {
        version: Some("0.0.103".to_owned()),
        flaw: None,
    };
    assert_eq!(check(ServerName::Ty, &ty), Ok(found));
}
