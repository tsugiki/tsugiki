use std::io::{BufReader, ErrorKind};

use super::{read, write};

fn frames(text: &str) -> BufReader<&[u8]> {
    BufReader::new(text.as_bytes())
}

#[test]
fn a_message_reads_back_as_it_was_written() {
    let mut written = Vec::new();
    write(&mut written, br#"{"jsonrpc":"2.0"}"#).unwrap();
    write(&mut written, "{\"é\":1}".as_bytes()).unwrap();
    assert_eq!(
        String::from_utf8(written.clone()).unwrap(),
        "Content-Length: 17\r\n\r\n{\"jsonrpc\":\"2.0\"}Content-Length: 8\r\n\r\n{\"é\":1}"
    );

    let mut reader = BufReader::new(written.as_slice());
    assert_eq!(read(&mut reader).unwrap().unwrap(), br#"{"jsonrpc":"2.0"}"#);
    assert_eq!(read(&mut reader).unwrap().unwrap(), "{\"é\":1}".as_bytes());
    assert_eq!(read(&mut reader).unwrap(), None);
}

#[test]
fn other_headers_and_the_name_s_case_dont_matter() {
    let mut reader = frames(
        "content-type: application/vscode-jsonrpc; charset=utf-8\r\nCONTENT-LENGTH:  2 \r\n\r\n{}",
    );
    assert_eq!(read(&mut reader).unwrap().unwrap(), b"{}");
}

#[test]
fn a_header_block_without_a_length_or_past_the_most_can_t_be_read_past() {
    for text in [
        "Content-Length: lots\r\nX-Other: 1\r\n\r\n{}",
        "X-Content-Length: 2\r\n\r\n{}",
        "Content-Length: 99999999999\r\n\r\n",
    ] {
        let error = read(&mut frames(text)).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidData, "{text:?}");
        assert!(
            error
                .to_string()
                .starts_with("no length this client can read in "),
            "{error}"
        );
    }
}

#[test]
fn a_stream_that_ends_inside_a_message_fails() {
    for text in ["Content-Length: 5\r\n", "Content-Length: 5\r\n\r\n{}"] {
        let error = read(&mut frames(text)).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::UnexpectedEof, "{text:?}");
    }
}

#[test]
fn a_header_that_isn_t_text_fails() {
    let mut reader = BufReader::new(&b"Content-Length: \xff\r\n\r\n"[..]);
    assert!(read(&mut reader).is_err());
}
