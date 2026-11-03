# A core library under a thin Tauri app

The Rust side is a Cargo workspace with two main crates. `crates/core` (package `tsugiki-core`) holds
everything that isn't the window: the mirror and git, the GitHub client, the local store, and the domain logic
(Issue trees, review state, Needs attention). `crates/app` (package `tsugiki-app`) is the Tauri app: it maps
commands and events onto the core and owns the windows. `crates/webview-keys` holds one unsafe call until
Tauri offers it ([ADR 0013](0013-webview2-browser-shortcuts-off-tsugiki-owns-its-keys.md)). The frontend is a
SvelteKit single-page app in `ui/`. The core is tested with plain `cargo test`, with no Tauri and no webview.
That matters because, after [ADR 0005](0005-token-stays-in-the-rust-core.md), almost all of the logic lives in
Rust.

## Considered Options

- **One crate per subsystem** (git, GitHub, store, domain). The compiler would enforce the boundaries from the start, but that means more manifests and more types crossing crates before the boundaries are known. The core can be split later if build times or boundaries call for it.
- **A single Tauri crate**, as in the prototypes. Simplest, but every test links Tauri, and the line between app and logic is only a convention.
- **Plain Svelte with Vite** instead of SvelteKit. Fewer parts, but SvelteKit is the path Tauri documents, and the prototypes' code uses it.

## Consequences

- The bundle identifier is `io.github.tsugiki`, and the product name is Tsugiki. The identifier can't change after the first release without losing settings and macOS keychain entries.
- The core computes its own folders with the `directories` crate, so users see `Tsugiki` (`%APPDATA%\Tsugiki`, `%LOCALAPPDATA%\Tsugiki`, `~/Library/Application Support/Tsugiki`), not Tauri's reverse-domain folder names.
- The executable is `tsg` ([ADR 0006](0006-named-tsugiki.md)). It is a single instance: a second `tsg` passes its arguments to the running app.
- `rust-toolchain.toml` pins an exact stable Rust with rustfmt and clippy, and `.node-version` pins Node 24 LTS. Upgrades are deliberate.
- A private root `package.json` makes `ui/` an npm workspace and holds the Tauri CLI and the scripts (`dev`, `build`, `check`, `lint`, `fmt`). The frontend uses ESLint (eslint-plugin-svelte, typescript-eslint), Prettier (prettier-plugin-svelte) and `svelte-check`.
- All frontend code, config and scripts are TypeScript: `.svelte` files use `<script lang="ts">`, and the configs are `vite.config.ts` and `eslint.config.ts`. Scripts are `.ts` files run directly by Node 24, which strips types, so they use only erasable syntax (no `enum` or `namespace`). A config whose tool can't load TypeScript stays `.js` with JSDoc types, so it is still type-checked.
- The prototypes stay on their own branches. Nothing is copied from them without a decision.
