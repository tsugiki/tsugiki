use rustls::pki_types::UnixTime;

use super::*;

/// A handshake that rustls failed, as ureq hands it back.
fn handshake_failed(error: CertificateError) -> ureq::Error {
    let tls = rustls::Error::InvalidCertificate(error);
    ureq::Error::Io(io::Error::new(io::ErrorKind::InvalidData, tls))
}

#[test]
fn a_certificate_outside_its_dates_is_about_the_clock() {
    // webpki's verifier, which ureq uses, gives the two with context.
    let time = UnixTime::since_unix_epoch(Duration::from_secs(1_800_000_000));
    let causes = [
        handshake_failed(CertificateError::ExpiredContext {
            time,
            not_after: UnixTime::since_unix_epoch(Duration::from_secs(1_700_000_000)),
        }),
        handshake_failed(CertificateError::NotValidYetContext {
            time,
            not_before: UnixTime::since_unix_epoch(Duration::from_secs(1_900_000_000)),
        }),
        handshake_failed(CertificateError::Expired),
        handshake_failed(CertificateError::NotValidYet),
    ];
    for cause in causes {
        assert_eq!(unreachable(cause), Unreachable::CertificateDates);
    }
}

#[test]
fn any_other_failure_is_offline() {
    let causes = [
        handshake_failed(CertificateError::UnknownIssuer),
        handshake_failed(CertificateError::ExpiredRevocationList),
        ureq::Error::Rustls(rustls::Error::InvalidCertificate(CertificateError::Expired)),
        ureq::Error::Io(io::Error::new(io::ErrorKind::TimedOut, "timed out")),
        ureq::Error::HostNotFound,
    ];
    for cause in causes {
        assert_eq!(unreachable(cause), Unreachable::Offline);
    }
}

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

/// A helper can leave git's tree while git runs: its parent ends first, as here, or it starts while
/// `taskkill /T` reads the tree (#208). On Windows, a process left running in the folder keeps it there.
#[test]
fn stopping_git_ends_what_it_started_though_it_left_gits_tree() {
    let folder = crate::testing::temp_path("stop-git");
    // An alias runs at the top of its repository, so this one needs its own.
    let _ = tsugiki_test_git::git(
        folder.parent().unwrap(),
        &["init", "-q", &folder.to_string_lossy()],
    );
    // A git, which holds the folder as Windows sees it, unlike MSYS's own programs such as `sleep`.
    let orphan = "sleep 60 | git hash-object --stdin";
    let hold = format!("alias.hold=!sh -c '{orphan} &'; echo started >&2; sleep 60");
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
    let _ = io::BufRead::read_line(&mut output, &mut line).unwrap();
    let _succeeded = git.stop();
    let (closing, closed) = std::sync::mpsc::channel();
    let _reading = thread::spawn(move || {
        let mut rest = Vec::new();
        closing
            .send(io::Read::read_to_end(&mut output, &mut rest).is_ok())
            .unwrap();
    });
    let closed = closed.recv_timeout(Duration::from_secs(30));
    let removed = std::fs::remove_dir_all(&folder);

    assert_eq!(line.trim_end(), "started");
    assert_eq!(closed, Ok(true), "the orphans have ended");
    assert!(removed.is_ok(), "{removed:?}");
}
