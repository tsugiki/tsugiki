# Named Tsugiki

The project is called Tsugiki (接ぎ木, grafting: joining a cut shoot onto a rooted stock) and lives at `github.com/tsugiki/tsugiki`. The working name, gh-rev, read like a `gh` CLI extension and was easily confused with gh-review, a separate tool Tsugiki does not replace. Grafting is what a core developer watches all day: a fix lands on main, and each **Backport** grafts it onto a maintenance branch.

Names that describe the job (prdeck, prinbox, patchdesk, reviewdesk, reviewdeck) are already used by other PR-inbox or review tools, and plain English words (loupe, docket, galley, scion) are taken everywhere. Some Japanese ones are taken by review tools too: sashiko and boro review patches, and akapen comments on lines. Tsugiki was free as a GitHub organization, on npm and on PyPI. Shinkokyu (深呼吸, a deep breath) was the runner-up.

## Consequences

- `tsugiki` on crates.io belongs to an unrelated HTML library. Tsugiki publishes an app, not a crate, so the Rust package takes a different name.
- The real app is named Tsugiki, with the identifier `io.github.tsugiki` and folders named `Tsugiki` ([ADR 0007](0007-core-library-and-thin-tauri-app.md)). The throwaway prototype keeps gh-rev.
- The wordmark is tsugi木: "tsugi" in Latin letters and the last syllable written as the kanji 木 (tree) in vermilion. Even people who don't read Japanese see a tree in 木, and 木 alone is the app icon.
- The command-line name is `tsg`. The meaning lives in the whole word, so short forms appear only where length matters.
