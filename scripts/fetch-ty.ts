// Fetches the ty we test Go to definition with into target/tools, checks its archive's checksum, and
// unpacks it there. The core's tests that start ty read it from that folder, and CI runs this before them.
// Tsugiki itself downloads nothing: the user supplies each language server (ADR 0020).
//
// It does nothing when that ty is there already.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readdirSync, renameSync, rmSync, statSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

/** The version the tests pin, which `crates/core/src/definitions/testing.rs` and the offline lookup test in
 * `crates/app/src/wiring/tests.rs` name too. */
const VERSION = "0.0.103";
const FOLDER = resolve(`target/tools/ty-${VERSION}`);

/** Each platform CI runs on: the archive's name and its SHA-256, from the release's checksums. */
const ARCHIVES: Record<string, { name: string; sha256: string } | undefined> = {
  "win32-x64": {
    name: "ty-x86_64-pc-windows-msvc.zip",
    sha256: "458db0c054572d7415ca991ada1f62c9596d48d0fe72b955dfb98034267530e2",
  },
  "darwin-arm64": {
    name: "ty-aarch64-apple-darwin.tar.gz",
    sha256: "dab6a1ec6ca888c25988ab8e8928f03f6663e970ff68c9e99d26b1debc0ea68f",
  },
  "linux-x64": {
    name: "ty-x86_64-unknown-linux-gnu.tar.gz",
    sha256: "9c29e9e082fa87f1be1015c2bdadd77524cebd4793512c1ccf72bc019befb9e2",
  },
};

const program = process.platform === "win32" ? "ty.exe" : "ty";

/** Windows' own tar by its full path, as Git's may come first on PATH and can't read a zip. */
function tar(): string {
  if (process.platform !== "win32") return "tar";
  return join(process.env.SystemRoot ?? "C:\\Windows", "System32", "tar.exe");
}

/** The path of the file named `name` somewhere under `folder`. */
function find(folder: string, name: string): string | undefined {
  for (const entry of readdirSync(folder)) {
    const path = join(folder, entry);
    if (statSync(path).isDirectory()) {
      const found = find(path, name);
      if (found !== undefined) return found;
    } else if (entry === name) {
      return path;
    }
  }
  return undefined;
}

async function main(): Promise<void> {
  if (existsSync(join(FOLDER, program))) return;
  const archive = ARCHIVES[`${process.platform}-${process.arch}`];
  if (archive === undefined) {
    throw new Error(`No ty archive is pinned for ${process.platform}-${process.arch}.`);
  }
  const url = `https://github.com/astral-sh/ty/releases/download/${VERSION}/${archive.name}`;
  const response = await fetch(url);
  if (!response.ok) throw new Error(`Fetching ${url} failed: ${String(response.status)}`);
  const bytes = Buffer.from(await response.arrayBuffer());
  const sha256 = createHash("sha256").update(bytes).digest("hex");
  if (sha256 !== archive.sha256) {
    throw new Error(`${archive.name} has SHA-256 ${sha256}, not ${archive.sha256}.`);
  }
  const unpacked = `${FOLDER}.unpacking`;
  rmSync(unpacked, { recursive: true, force: true });
  mkdirSync(unpacked, { recursive: true });
  const saved = join(unpacked, archive.name);
  writeFileSync(saved, bytes);
  const result = spawnSync(tar(), ["-xf", saved, "-C", unpacked], { stdio: "inherit" });
  if (result.status !== 0) throw new Error(`tar couldn't unpack ${archive.name}.`);
  const found = find(unpacked, program);
  if (found === undefined) throw new Error(`${archive.name} holds no ${program}.`);
  mkdirSync(FOLDER, { recursive: true });
  renameSync(found, join(FOLDER, program));
  rmSync(unpacked, { recursive: true, force: true });
  console.log(`ty ${VERSION} is in ${FOLDER}.`);
}

await main();
