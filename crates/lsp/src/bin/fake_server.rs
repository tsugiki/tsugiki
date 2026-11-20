//! A fake language server that runs as a process of its own, for the core's tests of what it does as it
//! starts, stops and replaces a server: a real ty or clangd only slows them down, and the in-memory fake in
//! `testing` can't be started from a program's path. A real process also holds its working folder, as a
//! real server does, so the tests that a checkout's folder goes once its server has ended still check
//! something on Windows.
//!
//! A test copies the program into a folder of its own, beside a script in [`FAKE_SERVER_SCRIPT`] that
//! `testing::fake_server_script` makes. With `--version` it prints the script's `version` and ends, and
//! otherwise it speaks LSP on its standard streams, whatever its other arguments. It names `version`
//! answering `initialize`, reports no problems in each document opened on it, answers the first
//! `textDocument/definition` with the first of `answers`, the next with the next and the rest with the
//! last, as [`reply_to`] says, and every other request with null. It ends at `exit`, or once its input
//! ends.

use std::io::{self, BufRead, Write};
use std::{env, fs};

use serde_json::{Value, json};
use tsugiki_lsp::testing::{FAKE_SERVER_SCRIPT, nth_answer, read_message, reply_to, write_message};

fn main() -> io::Result<()> {
    let script = env::current_exe()?.with_file_name(FAKE_SERVER_SCRIPT);
    let script: Value = serde_json::from_slice(&fs::read(script)?)?;
    let version = (script.get("version").and_then(Value::as_str)).unwrap_or_default();
    let answers = (script.get("answers").and_then(Value::as_array))
        .cloned()
        .unwrap_or_default();
    if env::args_os().any(|arg| arg == "--version") {
        return writeln!(io::stdout(), "{version}");
    }
    serve(
        &mut io::stdin().lock(),
        &mut io::stdout().lock(),
        version,
        &answers,
    )
}

/// Answers each request read from `input` on `output` until `exit` or the end of `input`, naming `version`
/// and giving the lookups `answers`.
fn serve(
    input: &mut impl BufRead,
    output: &mut impl Write,
    version: &str,
    answers: &[Value],
) -> io::Result<()> {
    let mut asked = 0;
    while let Some(message) = read_message(input)? {
        // What the client answers a request of the server's has no method, and this fake asks nothing.
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            continue;
        };
        let result = match method {
            "exit" => return Ok(()),
            "initialize" => {
                json!({"capabilities": {}, "serverInfo": {"name": "fake", "version": version}})
            }
            "textDocument/definition" => {
                asked += 1;
                nth_answer(answers, asked - 1)
            }
            // As a real server reports the problems of a document it opens, here none.
            "textDocument/didOpen" => {
                let uri = message.pointer("/params/textDocument/uri");
                let params = json!({"uri": uri, "diagnostics": []});
                let method = "textDocument/publishDiagnostics";
                let diagnostics = json!({"jsonrpc": "2.0", "method": method, "params": params});
                write_message(output, &diagnostics)?;
                continue;
            }
            _ => Value::Null,
        };
        // A notification gets no answer.
        if let Some(reply) = message.get("id").and_then(|id| reply_to(id, result)) {
            write_message(output, &reply)?;
        }
    }
    Ok(())
}
