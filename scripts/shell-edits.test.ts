import { describe, expect, it } from "vitest";

import { shellEdit } from "./shell-edits.ts";

describe("shellEdit", () => {
  it.each([
    ["sed -i", "sed -i 's/a/b/' crates/core/src/lib.rs"],
    ["sed -i", "cd /x/Work/gh-rev && sed -E --in-place 's/a/b/' ui/src/app.ts"],
    ["sed -i", "sed -Ei 's/a/b/' ./crates/core/src/lib.rs"],
    ["perl -i", "perl -0 -pi -e 's/a/b/' crates/core/src/diff.rs"],
    ["perl -i", "perl -i.bak -pe 's/a/b/' AGENTS.md"],
    ["a Python write", "python - <<'EOF'\np = 'crates/core/src/lib.rs'\nopen(p, 'w').write(s)\nEOF"],
    ["a Python write", "python -c \"open(os.path.join('ui', 'x.ts'), 'w+')\" # ui/x.ts"],
    ["a Python write", "python -c \"Path('scripts/merged.ts').write_text(s)\""],
    ["a Node write", "node -e \"fs.writeFileSync('ui/src/app.ts', s)\""],
    ["PowerShell's Set-Content", "Set-Content -Path docs/scope.md -Value $s"],
    ["tee", "echo x | tee crates/core/src/lib.rs"],
    ["a redirect", "cat > crates/core/src/new.rs <<'EOF'\nfn main() {}\nEOF"],
    ["a redirect", "printf '%s\\n' line >> justfile"],
    ["a redirect", "echo x >'/x/Work/gh-rev/.github/workflows/ci.yml'"],
    ["a redirect", "cd /tmp && echo x > /x/Work/gh-rev/justfile"],
  ])("refuses %s in %j", (how, command) => {
    expect(shellEdit(command)).toContain(`with ${how}.`);
  });

  it.each([
    "cargo test -p tsugiki-core --lib 2>&1 | rg 'crates/core'",
    "just check > .temp/scratchpad/check.log 2>&1; echo $?",
    "rg -n 'fn main' crates/core > .temp/scratchpad/hits.txt",
    "sed -n '1,40p' crates/core/src/lib.rs",
    "sed -i 's/a/b/' .temp/scratchpad/notes.md && rg x crates/core",
    "sed -n '1p' crates/a.rs\ngit grep -i foo",
    "perl -ne 'print if /fn/' crates/core/src/lib.rs",
    "perl -Mstrict -ne 'print if /fn/' crates/core/src/lib.rs",
    "python - <<'EOF'\nprint(open('crates/core/src/lib.rs').read())\nEOF",
    "cat <<'EOF' | rg x\nfn f() -> crates::Thing { x => 1 }\nEOF",
    "gh pr comment 12 --body-file - <<'EOF'\n> crates/core/src/diff.rs panics here\nEOF",
    "cd C:/Users/someone/scratchpad && printf 'x' > justfile",
    "echo x > .temp/scratchpad/docs/notes.md",
    "rg x crates | tee .temp/scratchpad/out.txt",
    "git mv crates/core/src/a.rs crates/core/src/a/mod.rs",
    "git commit -q -F .temp/scratchpad/commit.txt",
  ])("lets %j run", (command) => {
    expect(shellEdit(command)).toBeUndefined();
  });
});
