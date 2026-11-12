//! The core's first benchmark, run under CodSpeed on every PR (ADR 0017).

fn main() {
    divan::main();
}

#[divan::bench]
fn about() -> tsugiki_core::About {
    tsugiki_core::about(
        &tsugiki_core::version::Version::new(None),
        tsugiki_core::build::Build::Release,
    )
}
