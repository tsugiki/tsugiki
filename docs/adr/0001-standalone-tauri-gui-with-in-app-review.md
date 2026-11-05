# Standalone Tauri GUI with in-app review

Tsugiki is a standalone desktop app built on Tauri (Rust backend, Svelte 5 + TypeScript frontend), and
reviewing happens inside it with a custom virtualized diff renderer. We did not extend gh-dash and did not
hand off to gh-review or the GitHub web UI. The core need is a Workbench of Issue trees with per-PR review
state, and gh-dash's flat, query-per-section lists cannot express that. GitHub's web UI is too slow for daily
review.

## Considered Options

- **gh-dash extension, or a gh-dash companion launched by a key.** gh-dash would still own the home screen, so
  Issue grouping and the Stale/New activity badges would only show after drilling into one PR.
- **TUI with hand-off to gh-review.** gh-review stays a separate tool, and we want the review itself to be
  fast and integrated with review state.
- **egui (pure Rust).** Simpler for the diff, but much worse for everything around it. PR descriptions and
  comments are GitHub Markdown, which GitHub's API returns pre-rendered as `bodyHTML`; a webview shows that
  exactly as github.com does. A webview also gives native text selection across diff rows and a proper comment
  composer (IME, spell check). The custom diff renderer replaces a code editor, not the webview.
- **Monaco or CodeMirror for diffs.** Both are editors built around one document each. A review is one
  continuous, read-and-annotate scroll across many files, with collapsed Generated files, NEWS entries and
  inline threads as rows.

## Consequences

- v1 targets Windows (WebView2) and macOS (WKWebView). Linux (WebKitGTK) is not a v1 target.
- Syntax highlighting runs in Rust, and the frontend receives ready-made token spans.
- The diff renderer virtualizes at row level, and the rows stay in Rust. The frontend asks for row ranges in
  chunks over IPC, and Rust highlights files only when they are first asked for. The webview only ever holds
  the rows near the viewport. The prototype on branch `prototype/diff-renderer` compared this design with two
  others. With 150k rows it dropped no frames on WebView2 or WKWebView. It reached first paint in 1.4 s on
  Windows and 0.8 s on macOS, against 5.9 s and 3.1 s for highlighting everything up front, and on Windows it
  used 16 MB of JS heap against 37 MB. Inserting each file's rows as a block of HTML caused freezes of up to
  0.9 s, and halved the frame rate on macOS. One cost is still open. Jumping into a large file that hasn't
  been highlighted yet takes up to 214 ms on Windows and 117 ms on macOS, because the whole file is
  highlighted at that moment. Highlighting per chunk, or in the background after first paint, should fix it.
- Tsugiki writes to GitHub: review comments, Pending reviews, submitting reviews, replies, resolving threads,
  suggestions, and PR-level comments. Merging, labels and reviewer requests stay on GitHub.
