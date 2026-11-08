# Contributing

## Run the app

Install [`just`](https://github.com/casey/just) with `cargo install just --locked --version 1.59.0`, then run:

```
just install
just dev
```

`just dev` starts the Vite dev server and the app, and reloads the app as you edit. `just` lists the other
commands.

A debug build loads its UI from the dev server at `http://localhost:1420`. `cargo run` starts only the app, so
its window shows "can't reach this page". To use `cargo run`, for example with a debugger, start the server
first in another terminal:

```
npm run dev -w ui
cargo run
```

## Run the checks

`just check` runs every test CI runs. Its gates call three cargo tools besides `just`. Install the versions
that `.github/workflows/ci.yml` pins:

```
cargo install cargo-llvm-cov --locked --version 0.9.1
cargo install cargo-deny --locked --version 0.20.2
cargo install cargo-machete --locked --version 0.9.2
```

A PR that bumps one of these in `ci.yml` changes this list too.

## Add a language to the highlighter

[docs/highlighting.md](docs/highlighting.md) says how the Files tab colors code, and walks through adding a
language: a crate, an entry in a table, and a sample file.
