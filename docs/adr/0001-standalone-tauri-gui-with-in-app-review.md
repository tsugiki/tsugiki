# Standalone Tauri GUI with in-app review

gh-rev is a standalone desktop app built on Tauri (Rust backend, Svelte 5 + TypeScript frontend), and reviewing happens inside it with a custom virtualized diff renderer. We did not extend gh-dash and did not hand off to gh-review or the GitHub web UI. The core need is a Workbench of Issue trees with per-PR review state, and gh-dash's flat, query-per-section lists cannot express that. GitHub's web UI is too slow for daily review.

## Considered Options

- **gh-dash extension, or a gh-dash companion launched by a key.** gh-dash would still own the home screen, so Issue grouping and the Stale/New activity badges would only show after drilling into one PR.
- **TUI with hand-off to gh-review.** gh-review stays a separate tool, and we want the review itself to be fast and integrated with review state.
- **egui (pure Rust).** Simpler for the diff, but much worse for everything around it. PR descriptions and comments are GitHub Markdown, which GitHub's API returns pre-rendered as `bodyHTML`; a webview shows that exactly as github.com does. A webview also gives native text selection across diff rows and a proper comment composer (IME, spell check). The custom diff renderer replaces a code editor, not the webview.
- **Monaco or CodeMirror for diffs.** Both are editors built around one document each. A review is one continuous, read-and-annotate scroll across many files, with collapsed Generated files, NEWS entries and inline threads as rows.

## Consequences

- v1 targets Windows (WebView2) and macOS (WKWebView). The diff renderer's performance on WKWebView must be checked early. Linux (WebKitGTK) is not a v1 target.
- Syntax highlighting runs in Rust, and the frontend receives ready-made token spans.
- gh-rev writes to GitHub: review comments, Pending reviews, submitting reviews, replies, resolving threads, suggestions, and PR-level comments. Merging, labels and reviewer requests stay on GitHub.
