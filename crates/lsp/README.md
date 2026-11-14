# tsugiki-lsp

This crate talks to a language server, such as ty, clangd or rust-analyzer, over the server's standard input
and output. Go to definition and the Definition peek ask a server where a name is defined (ADR 0020), and
this is the client they ask through.

## What it does

- Starts a server from a program, its arguments and a working folder, with no console window on Windows.
  The processes the server starts end with it. On Windows a job object ends them all when Tsugiki's process
  ends, even in a crash. `Server::stop` asks the server to stop with `shutdown` and `exit`, and kills it
  if it doesn't agree, or doesn't exit, within the time the caller gives. Dropping the `Server` kills it.
- Frames each message with its `Content-Length` header, and speaks JSON-RPC: request ids, answers, errors,
  notifications, and `$/cancelRequest` when the caller cancels a request.
- Sends `initialize` with what the client can do and the server's own options, waits for the answer, then
  sends `initialized`. A started server offers nothing else until then, as LSP lets nothing reach a server
  first. The client answers the server's `workspace/configuration` from settings the caller gives it.
- Opens documents with `didOpen`, sends each change as the full text with `didChange`, and closes them with
  `didClose`. It counts each document's versions.
- Asks `textDocument/definition`, and returns the locations or links the server answers with.
- Hands the caller `$/progress` and `textDocument/publishDiagnostics` as events. When the server exits, or
  crashes, the caller gets an event with its exit code and the end of its error output.
- Converts LSP's columns, which count UTF-16 code units, to and from bytes in a line of text.

## What it doesn't know

It knows nothing of PRs, Repositories, Mirrors, the Lookup checkout or the database. The core decides which
server to start and when, which documents to open with what text, and what to ask. It downloads nothing:
the user supplies each server.

## How it works

One thread owns each server. It alone sends the server messages and keeps the requests that wait for an
answer. The caller's `Server`, `Pending` and `Document` handles send it messages. A writer thread writes to
the server, so a server that stops reading holds up nobody, and two more threads read the server's output
and its error output, parse each message into a typed value as it arrives, and send it to the owner. An
answer that doesn't parse fails its own request, and the client goes on.

When the server's output ends, the client waits for the server's process alone, then ends what the server
left running. Once the caller has asked a server to stop, the owner thread waits for it to agree and then
to exit, reading what it says and writing nothing more, and kills it when either wait runs out.

The client reads and writes plain streams. `src/process.rs` is the thin layer that starts a real process,
and the tests replace it: each one plays the server itself over in-memory pipes (`src/testing.rs`). That
file is the one the coverage gate leaves out. `tests/crash.rs` runs real processes on Windows: it kills a
parent that started a server, and checks the server and the process it started have ended.

process-wrap puts each server in a job object of its own, which doesn't kill what is in it when Tsugiki's
process ends. So `src/process.rs` also puts the server, while it is still suspended, in a second job object
that does, through the `win32job` crate, so the crate keeps no unsafe code of its own.

The crate has its own LSP types on `serde`, not the `lsp-types` crate. In a debug build `lsp-types` took
5.9 s on its own, for the dozen types this client reads.
