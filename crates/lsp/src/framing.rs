//! LSP's base protocol: a `Content-Length` header, a blank line, then that many bytes of JSON.

use std::io::{self, BufRead, Write};

#[cfg(test)]
mod tests;

/// The most one message may hold. A server that claims more has lost its framing, and reading on would
/// hold that much memory.
const MOST_BYTES: usize = 64 * 1024 * 1024;

/// Reads one message's body, or `None` when the stream ends between messages.
///
/// A stream that ends inside a message fails, and so does a header block without a `Content-Length` we can
/// read, or with one past [`MOST_BYTES`]: without a length, the next message can't be found, so nothing
/// after it can be read.
pub(crate) fn read(reader: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut length = None;
    let mut headers = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            if headers.is_empty() {
                return Ok(None);
            }
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        let header = line.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse::<usize>().ok();
        }
        headers.push_str(&line);
    }
    let length = length
        .filter(|length| *length <= MOST_BYTES)
        .ok_or_else(|| {
            let text = format!("no length this client can read in {headers:?}");
            io::Error::new(io::ErrorKind::InvalidData, text)
        })?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}

/// Writes one message with its header, and flushes it so the server sees it now.
pub(crate) fn write(writer: &mut impl Write, body: &[u8]) -> io::Result<()> {
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(body)?;
    writer.flush()
}
