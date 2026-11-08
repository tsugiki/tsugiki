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
