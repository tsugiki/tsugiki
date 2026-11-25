// Finds the language servers we test Go to definition with. It fetches ty and clangd into target/tools,
// checks each archive's checksum, and unpacks it there, unless it is there already. rust-analyzer comes with
// the toolchain that `rust-toolchain.toml` pins, which rustup installs with its components. Tsugiki itself
// downloads nothing: the user supplies each language server (ADR 0020).
//
// It also builds `tsugiki-fake-server`, the fake server some tests start in a real one's place.
//
// The Rust tests that start a server read its path from `TSUGIKI_TY`, `TSUGIKI_RUST_ANALYZER`,
// `TSUGIKI_CLANGD` or `TSUGIKI_FAKE_SERVER`. Given a command, this runs it with each set, as
// `npm run cargo-test` and `npm run rust-coverage` do. Alone, it prints the paths to set them to.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readdirSync, renameSync, rmSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

/** An archive for one platform: its name and its SHA-256, from the release's checksums. */
interface Archive {
  name: string;
  sha256: string;
}

/** A server the tests pin, which a release on GitHub holds an archive of for each platform CI runs on. */
interface Tool {
  name: string;
  version: string;
  /** The folder of the release's archives. */
  releases: string;
  archives: Record<string, Archive | undefined>;
  /** The program's path in the folder the tool unpacks to. */
  program: string;
  /**
   * Whether the tool keeps the whole folder its program is in, as clangd reads its built-in headers from
   * `lib/clang` beside `bin`, or only the program.
   */
  folder: boolean;
}

const exe = process.platform === "win32" ? ".exe" : "";

const TY: Tool = {
  name: "ty",
  version: "0.0.103",
  releases: "https://github.com/astral-sh/ty/releases/download/0.0.103",
  archives: {
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
  },
  program: `ty${exe}`,
  folder: false,
};

const CLANGD: Tool = {
  name: "clangd",
  version: "23.1.0",
  releases: "https://github.com/clangd/clangd/releases/download/23.1.0",
  archives: {
    "win32-x64": {
      name: "clangd-windows-23.1.0.zip",
      sha256: "23412a240756a162e7b98a282f36aa2a23a88db5ce16a0cbc4fef7253768c810",
    },
    "darwin-arm64": {
      name: "clangd-mac-23.1.0.zip",
      sha256: "1082e6638223b785ca2daf0939f13afcd0bb95c84ee9a4bbaff4745365159253",
    },
    "linux-x64": {
      name: "clangd-linux-23.1.0.zip",
      sha256: "e53b1a96196095faedb7642cf64964f7fb9ad4a0c1f00dd2c172a3d9dcbafdfd",
    },
  },
  program: join("bin", `clangd${exe}`),
  folder: true,
};

/** The folder `tool` unpacks to. */
function folderOf(tool: Tool): string {
  return resolve(`target/tools/${tool.name}-${tool.version}`);
}

/** The command that unpacks `archive` into `folder`. */
function unpacker(archive: string, folder: string): [string, string[]] {
  // Windows' own tar by its full path, as Git's may come first on PATH and can't read a zip.
  if (process.platform === "win32") {
    const tar = join(process.env.SystemRoot ?? "C:\\Windows", "System32", "tar.exe");
    return [tar, ["-xf", archive, "-C", folder]];
  }
  if (archive.endsWith(".zip")) return ["unzip", ["-q", archive, "-d", folder]];
  return ["tar", ["-xf", archive, "-C", folder]];
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

/** Fetches and unpacks `tool`, unless it is there already, and returns its program's path. */
async function fetchTool(tool: Tool): Promise<string> {
  const folder = folderOf(tool);
  const program = join(folder, tool.program);
  if (existsSync(program)) return program;
  const platform = `${process.platform}-${process.arch}`;
  const archive = tool.archives[platform];
  if (archive === undefined) throw new Error(`No ${tool.name} archive is pinned for ${platform}.`);
  const url = `${tool.releases}/${archive.name}`;
  const response = await fetch(url);
  if (!response.ok) throw new Error(`Fetching ${url} failed: ${String(response.status)}`);
  const bytes = Buffer.from(await response.arrayBuffer());
  const sha256 = createHash("sha256").update(bytes).digest("hex");
  if (sha256 !== archive.sha256) {
    throw new Error(`${archive.name} has SHA-256 ${sha256}, not ${archive.sha256}.`);
  }
  const unpacked = `${folder}.unpacking`;
  rmSync(unpacked, { recursive: true, force: true });
  mkdirSync(unpacked, { recursive: true });
  const saved = join(unpacked, archive.name);
  writeFileSync(saved, bytes);
  const [command, args] = unpacker(saved, unpacked);
  const result = spawnSync(command, args, { stdio: "inherit" });
  if (result.status !== 0) throw new Error(`${command} couldn't unpack ${archive.name}.`);
  const name = tool.program.split(/[\\/]/).at(-1) ?? tool.program;
  const found = find(unpacked, name);
  if (found === undefined) throw new Error(`${archive.name} holds no ${name}.`);
  rmSync(folder, { recursive: true, force: true });
  if (tool.folder) {
    // The program sits as deep in the archive's folder as in the one it unpacks to.
    const depth = tool.program.split(/[\\/]/).length;
    const root = Array.from({ length: depth }).reduce<string>((path) => dirname(path), found);
    renameSync(root, folder);
  } else {
    mkdirSync(folder, { recursive: true });
    renameSync(found, program);
  }
  rmSync(unpacked, { recursive: true, force: true });
  console.log(`${tool.name} ${tool.version} is in ${folder}.`);
  return program;
}

/** The rust-analyzer in the toolchain `rust-toolchain.toml` pins, which rustup installs as it starts. */
function rustAnalyzer(): string {
  const which = spawnSync("rustup", ["which", "rust-analyzer"], { encoding: "utf8" });
  if (which.status !== 0) throw new Error(`rustup has no rust-analyzer: ${which.stderr}`);
  return which.stdout.trim();
}

/** What cargo says of a target it built, as `--message-format=json` writes it, as far as this reads it. */
interface Artifact {
  reason: string;
  target?: { name: string };
  executable?: string | null;
}

/**
 * Builds the fake language server in `tsugiki-lsp`, which the core's tests start where a real one would only
 * slow them down, and returns its program's path. It builds apart from the tests, so coverage doesn't count
 * it.
 */
function fakeServer(): string {
  const name = "tsugiki-fake-server";
  const args = ["build", "-p", "tsugiki-lsp", "--features", "testing", "--bin", name];
  // cargo writes a line of JSON for each crate it builds, which outgrows the default 1 MiB.
  const build = spawnSync("cargo", [...args, "--message-format=json-render-diagnostics"], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
    maxBuffer: 64 * 1024 * 1024,
  });
  if (build.status !== 0) throw new Error(`cargo couldn't build ${name}.`);
  const built = build.stdout
    .split("\n")
    .filter((line) => line.startsWith("{"))
    .map((line) => JSON.parse(line) as Artifact)
    .find((artifact) => artifact.reason === "compiler-artifact" && artifact.target?.name === name);
  const program = built?.executable;
  if (program === undefined || program === null) throw new Error(`cargo built no ${name}.`);
  return program;
}

const servers = {
  TSUGIKI_TY: await fetchTool(TY),
  TSUGIKI_RUST_ANALYZER: rustAnalyzer(),
  TSUGIKI_CLANGD: await fetchTool(CLANGD),
  TSUGIKI_FAKE_SERVER: fakeServer(),
};
const [command, ...args] = process.argv.slice(2);
if (command === undefined) {
  const set = Object.entries(servers).map(([name, path]) => `${name}=${path}`);
  console.log(`Set ${set.join(", ")} to run the Rust tests with cargo alone.`);
  console.log(
    "Run this again after a change to the fake server: `cargo nextest run -p tsugiki-core` doesn't build it.",
  );
} else {
  const run = spawnSync(command, args, { stdio: "inherit", env: { ...process.env, ...servers } });
  if (run.error !== undefined) throw run.error;
  process.exit(run.status ?? 1);
}
