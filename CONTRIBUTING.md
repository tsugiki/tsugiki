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
