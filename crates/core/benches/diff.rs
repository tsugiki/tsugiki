//! A PR's diff as the Files tab reads it: parsing what `git diff` printed, and the chunks the webview asks
//! for (ADR 0001). The diff's first rows have 1 s, and a jump into a file 100 ms (ADR 0017).

use tsugiki_core::diff::Diff;

fn main() {
    divan::main();
}

/// Files in the diff, about as many as a large backport to a CPython branch touches.
const FILES: u32 = 1_100;

/// Hunks in each file. With the rows of each hunk, the diff holds about 150,000 rows.
const HUNKS: u32 = 4;

/// The rows the webview asks for at a time, as `CHUNK` in `pr-files.svelte.ts`.
const CHUNK: u32 = 512;

#[divan::bench]
fn parse(bencher: divan::Bencher<'_, '_>) {
    let (numstat, patch) = printed();
    bencher.bench(|| Diff::parse(&numstat, &patch));
}

/// A chunk from the middle of the diff, turned into the JSON the webview receives.
#[expect(
    clippy::expect_used,
    reason = "a benchmark that timed an empty diff would measure nothing the user waits for"
)]
#[divan::bench]
fn chunk(bencher: divan::Bencher<'_, '_>) {
    let (numstat, patch) = printed();
    let diff = Diff::parse(&numstat, &patch).expect("the printed diff parses");
    assert!(diff.row_count() > 140_000, "{} rows", diff.row_count());
    let start = diff.row_count() / 2;
    bencher.bench(|| serde_json::to_string(diff.rows(start..start + CHUNK)));
}

/// What `git diff --numstat -z` and `git diff` print for `FILES` files of `HUNKS` hunks each: each hunk swaps
/// 12 lines for 15 between 3 lines of context on each side.
fn printed() -> (String, String) {
    let mut numstat = Vec::new();
    let mut patch = Vec::new();
    for file in 0..FILES {
        let path = format!("Lib/test/test_module_{file}.py");
        numstat.push(format!("{}\t{}\t{path}\0", 15 * HUNKS, 12 * HUNKS));
        patch.push(format!("diff --git a/{path} b/{path}"));
        patch.push("index 0123456..89abcde 100644".to_owned());
        patch.push(format!("--- a/{path}"));
        patch.push(format!("+++ b/{path}"));
        for hunk in 0..HUNKS {
            let old = 1 + hunk * 100;
            let new = old + hunk * 3;
            patch.push(format!(
                "@@ -{old},18 +{new},21 @@ def test_case_{hunk}(self):"
            ));
            patch.extend(
                (0..3).map(|line| {
                    format!("         self.assertEqual(value_{line}, expected_{line})")
                }),
            );
            patch.extend(
                (0..12)
                    .map(|line| format!("-        result = compute(argument_{line}, flag=False)")),
            );
            patch.extend((0..15).map(|line| {
                format!("+        result = compute(argument_{line}, flag=True, strict=True)")
            }));
            patch.extend(
                (0..3).map(|line| format!("         self.assertTrue(check_{line}(result))")),
            );
        }
    }
    patch.push(String::new());
    (numstat.concat(), patch.join("\n"))
}
