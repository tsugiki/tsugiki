use std::time::Duration;

use super::*;

#[test]
fn a_failed_git_says_its_reason_rather_than_the_hint_after_it() {
    let dubious = "fatal: detected dubious ownership in repository at 'D:/cpython'\n\
            To add an exception for this directory, call:\n\
            \n\
            \tgit config --global --add safe.directory D:/cpython\n";

    assert_eq!(
        gist(dubious),
        "fatal: detected dubious ownership in repository at 'D:/cpython'"
    );
    assert_eq!(gist("warning: one\nerror: two\nhint: three"), "error: two");
    assert_eq!(gist("Connection reset\nby peer"), "by peer");
    assert_eq!(gist(""), "");
}

#[test]
fn git_s_batch_gives_each_blob_s_bytes_and_nothing_for_the_rest() {
    let output = b"1111 blob 3\nab\n\n\
        2222:Lib/x.py missing\n\
        3333 tree 4\n\0\0\0\0\n\
        4444:Lib/spaced name.py missing\n\
        5555 blob 20\ncut short";

    let blobs = parse_batch(output);

    assert_eq!(blobs, [Some(b"ab\n".to_vec()), None, None, None]);
}

#[test]
fn reading_no_blob_asks_git_for_nothing() {
    assert!(read_blobs(Path::new("no-folder"), &[]).unwrap().is_empty());
}

#[test]
fn a_name_with_a_line_break_gets_nothing_and_the_blobs_after_it_keep_their_places() {
    let mirrored = crate::diff::testing::Mirrored::new("line-break-name");
    let head = mirrored.head.as_str();
    let names = [
        format!("{head}:Tools/run.sh"),
        format!("{head}:Lib/x.py\n{head}:Lib/ast.py"),
        format!("{head}:Lib/spaced name ü.py"),
    ];
    let names: Vec<&str> = names.iter().map(String::as_str).collect();

    let blobs = read_blobs(&mirrored.mirror, &names).unwrap();

    assert_eq!(
        blobs,
        [Some(b"echo hi\n".to_vec()), None, Some(b"pass\n".to_vec())]
    );
}

#[test]
fn names_git_would_read_as_others_ask_git_for_nothing() {
    let names = ["1111:Lib/x.py\nLib/y.py", "1111:Lib/x.py\r"];

    let blobs = read_blobs(Path::new("no-folder"), &names).unwrap();

    assert_eq!(blobs, [None, None]);
}

/// A helper can leave git's tree while git runs: its parent ends first, as here, or it starts while
/// `taskkill /T` reads the tree (#208). On Windows, a process left running in the folder keeps it there. On
/// macOS, a process that starts while the kill runs can outlive it (#309), so the alias goes on starting
/// them after it says it has started.
#[test]
fn stopping_git_ends_what_it_started_though_it_left_gits_tree() {
    let folder = crate::testing::temp_path("stop-git");
    // An alias runs at the top of its repository, so this one needs its own.
    tsugiki_test_git::run_git(
        folder.parent().unwrap(),
        &["init", "-q", &folder.to_string_lossy()],
    );
    // A git, which holds the folder as Windows sees it, unlike MSYS's own programs such as `sleep`.
    let orphan = "sleep 60 | git hash-object --stdin";
    let late = "for i in 1 2 3 4 5 6 7 8 9 10; do sleep 60 & done";
    let hold = format!("alias.hold=!sh -c '{orphan} &'; echo started >&2; {late}; sleep 60");
    let args = [
        OsStr::new("-C"),
        folder.as_os_str(),
        OsStr::new("-c"),
        OsStr::new(&hold),
        OsStr::new("hold"),
    ];

    let (mut git, output) = start_git(&args, b"").unwrap();
    // The orphans hold the error output too, so it closes only once they have ended, as the mirror's
    // worker waits for it to.
    let mut output = io::BufReader::new(output);
    let mut line = String::new();
    #[expect(unused_results, reason = "the test checks the line itself at the end")]
    io::BufRead::read_line(&mut output, &mut line).unwrap();
    git.stop();
    let (closing, closed) = std::sync::mpsc::channel();
    #[expect(
        unused_results,
        reason = "a join would hang on the orphans this test checks for, where the timeout fails it"
    )]
    thread::spawn(move || {
        let mut rest = Vec::new();
        closing
            .send(io::Read::read_to_end(&mut output, &mut rest).is_ok())
            .unwrap();
    });
    let closed = closed.recv_timeout(Duration::from_secs(30));
    // Once, at once: the guard would wait for an orphan that outlives the output by a few seconds.
    let removed = std::fs::remove_dir_all(&folder);

    assert_eq!(line.trim_end(), "started");
    assert_eq!(closed, Ok(true), "the orphans have ended");
    assert!(removed.is_ok(), "{removed:?}");
}
