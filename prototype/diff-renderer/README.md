# PROTOTYPE: diff renderer spike (throwaway)

This prototype lives only on the `prototype/diff-renderer` branch, and none of its code is meant to reach `main`.

**Question.** ADR 0001 depends on a custom virtualized diff renderer: one continuous scroll over every file of a large CPython PR, inside a Tauri webview (WebView2 on Windows, WKWebView on macOS), with syntax highlighting done in Rust. Does it stay smooth with tens of thousands of diff lines, including collapsed Generated files and inline comment threads?

**Plan.** There are three rendering strategies, chosen with `?variant=`. The ← / → keys or the bottom bar switch between them.

| Variant | Who virtualizes | Where the rows live |
|---|---|---|
| **A**: row virtualizer, all rows in JS | A JS virtual list. Code rows are 20 px tall, and comment threads are measured with a ResizeObserver and indexed with a Fenwick tree. | Rust highlights everything up front and sends it in one IPC call. |
| **B**: file blocks with `content-visibility` | Nobody at row level. Each file is a `<section>` whose rows are turned into plain HTML when it comes within 3000 px of the viewport, and dropped again once it is far away. | All rows are in JS, as in A. |
| **C**: row virtualizer, rows fetched from Rust | The same virtual list as A. | Rows stay in Rust. Files are highlighted lazily, and rows are fetched in chunks of 512 over IPC. |

**Data.** The prototype runs `git diff` between real CPython tags in a local clone. It only reads from the clone and never writes to it.

- `big`: `v3.15.0b4..v3.15.0rc1`, about 323 files and 8,300 changed lines.
- `huge`: `v3.15.0b1..v3.15.0rc1`, about 1,118 files and 102,000 changed lines.
- `custom`: whatever range `GHREV_PROTO_RANGE` is set to.

Generated files are taken from `.gitattributes` at the head commit and start out collapsed. The comment threads are synthetic HTML, a stand-in for GitHub's `bodyHTML`, placed after roughly one code line in 140.

## Run

```sh
cd prototype/diff-renderer                    # every command below runs from here, not the repo root
npm install
# Path to your CPython clone. Defaults to X:/Work/cpython/main.
export GHREV_PROTO_REPO=~/src/cpython        # PowerShell: $env:GHREV_PROTO_REPO="..."
npm run proto                                 # dev mode, interactive
```

The HUD in the top-right corner shows live frame times, the DOM node count, the cost of each loading step, and IPC statistics. It also has buttons that run repeatable benchmarks: scrolling at 2k px/s, flinging at 20k px/s, and 30 random jumps.

### Benchmarking the release build

```sh
npm run proto:release
GHREV_PROTO_AUTOBENCH=1 GHREV_PROTO_LOG=$PWD/bench-results.txt ./src-tauri/target/release/diff-renderer
```

This runs every combination of variant (A, B, C) and dataset (big, huge), appends one JSON line per measurement to the log file, and then quits. It takes about 6 minutes. Keep the window in the foreground the whole time: a hidden or minimized window slows down `requestAnimationFrame`, which skews the numbers. On macOS the binary is at `src-tauri/target/release/diff-renderer`. On Windows it is `src-tauri\target\release\diff-renderer.exe`.

## Known simplifications

- Highlighting runs over the diff lines only, with one parse state for the old side and one for the new side, rather than over the full files. A comment or string that opens before a hunk starts can be highlighted wrongly.
- Only unified view is implemented. There is no side-by-side view, no dark theme, and nothing is evicted from the chunk cache in C.
- Lines are not wrapped. The whole list scrolls horizontally, with its width set by the longest line in the diff.

## Verdict on Windows (WebView2, 75 Hz display), 2026-11-01

The raw numbers are in [bench-results.txt](bench-results.txt). At 75 Hz one frame is 13.3 ms, so a p95 of 13.4 ms means no frames were dropped.

| | A: rows in JS | B: file blocks | C: rows from Rust |
|---|---|---|---|
| big: load to first paint | 1.45 s | 1.28 s | **0.64 s** |
| huge: load to first paint | 5.86 s (4.2 s highlight, 0.3 s IPC of 27 MB) | 5.87 s | **1.40 s** |
| huge: fling at 20k px/s, p95 / worst frame | 13.4 / 13.6 ms | 13.5 / **853 ms** | 13.4 / 13.5 ms |
| huge: 30 random jumps, avg / worst | 26 / 28 ms | 161 / **912 ms** | 67 / 214 ms |
| huge: fling with Generated files expanded | 13.4 / 13.6 ms | 13.5 / 839 ms | 13.4 / 13.6 ms |
| blank frames while scrolling | 0 | 0 | 0 |
| huge: JS heap | 37 MB | 42 MB | **16 MB** |

- **A row-level virtual list in a webview is smooth.** With 150,279 rows, 970 measured comment threads, and every Generated file expanded, it never dropped a frame. DOM size stays at about 1,200 nodes whatever the size of the PR. This settles the question behind ADR 0001 on Windows.
- **B is out.** When a large file comes into view, its rows are inserted as HTML all at once, which produces frames of 0.8 to 0.9 s on the huge dataset.
- **C is the shape to build: rows stay in Rust, files are highlighted lazily, and rows reach JS in chunks.** It loads 4× faster than A on the huge dataset, uses less than half the heap, and scrolls just as smoothly. Its weak spot is the cold jump. When a jump lands in a large file that hasn't been highlighted yet, Rust highlights the whole file before replying, which made the worst jump 214 ms and the p95 chunk fetch 155 ms. Two ways to fix it: highlight per chunk rather than per file, or keep highlighting in a background thread after the first paint.
- **Eager highlighting doesn't scale.** syntect manages about 35k rows/s, which works out to 4.2 s for the huge dataset.

## Verdict on macOS (WKWebView, 60 Hz display), 2026-11-01

The raw numbers are in [bench-results-macos.txt](bench-results-macos.txt). At 60 Hz one frame is 16.7 ms. WebKit rounds frame timestamps to whole milliseconds, so a p95 of 17 or 18 ms means no frames were dropped. WebKit doesn't report JS heap size, so there is no memory row.

| | A: rows in JS | B: file blocks | C: rows from Rust |
|---|---|---|---|
| big: load to first paint | 0.79 s | 0.76 s | **0.16 s** |
| huge: load to first paint | 3.14 s (2.3 s highlight) | 2.90 s | **0.78 s** |
| big: fling at 20k px/s, p95 / worst frame | 18 / 36 ms | 17 / 34 ms | 17 / 18 ms |
| huge: fling at 20k px/s, p95 / worst frame | 18 / 18 ms | **48 / 494 ms**, 32 fps | 17 / 18 ms |
| huge: 30 random jumps, avg / worst | 33 / 37 ms | 143 / 691 ms | 49 / 117 ms |
| huge: fling with Generated files expanded | 18 / 22 ms | 48 / 494 ms | 17 / 19 ms |
| huge: chunk fetch, avg / p95 | n/a | n/a | 20 / 82 ms |
| blank frames while scrolling | 0 | 0 | 0 |

- **WKWebView matches WebView2.** Both A and C stay smooth at 150k rows, so the macOS question behind ADR 0001 is settled.
- **C is the only variant that never dropped a frame on either platform.** A lost a couple of frames flinging through `big` on macOS.
- **B is worse on macOS than on Windows.** Flinging through `huge` runs at half the frame rate.
- **The cold-jump cost is still there, but smaller:** 117 ms at worst, against 214 ms on Windows, because the Mac highlights about twice as fast.

**Not yet measured:**
- **The full length of the huge dataset.** Each scroll benchmark is capped at 10 s, so it covers only the first 200k px of about 3M. The random jumps do sample the whole list.
- **Text selection across recycled rows.** This needs a manual test.
- **Displays faster than 75 Hz,** where there would be headroom to measure.

## Text selection (manual check, 2026-11-01)

Selecting text in the diff works on both platforms. On macOS the selection took in the line-number gutter as well, because WKWebView ignores unprefixed `user-select: none`. Line numbers are now CSS `::before` content taken from a `data-n` attribute. Generated content is never selected or copied on either engine, so the real renderer should draw its gutter the same way.

After the fix, Windows was checked by hand: selecting works, and copied text has no line numbers. macOS still needs the same check.
