// Zips the Windows portable copy from what `npx tauri build` left in target/release: `tsg.exe`, and an empty
// `data` folder that makes it portable (ADR 0014).
//
// With `--try` it then unpacks the zip and starts it, to show that this zip runs and keeps its data in its
// own folder. That opens a real window, so the release workflow passes it and a person running this doesn't.
import { type ChildProcess, spawn, spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { setTimeout as wait } from "node:timers/promises";

import { workspaceVersion } from "./release-notes.ts";

const BUILT = resolve("target/release/tsg.exe");
const STAGE = resolve(".temp/portable");
const UNPACKED = resolve(".temp/portable-try");
const OUTPUT = resolve("target/release/bundle/portable");
const WAIT_FOR_DATABASE_MS = 60_000;
/** The database appears before the page loads, so a copy that crashes after that has to show in this time. */
const KEEP_RUNNING_MS = 10_000;

/** Windows' own, by its full path: Git's `tar` may come first on PATH, and it can't write a zip. */
function tar(...args: string[]): void {
  const program = join(process.env.SystemRoot ?? "C:\\Windows", "System32", "tar.exe");
  const result = spawnSync(program, args, { stdio: "inherit" });
  if (result.status !== 0) {
    throw new Error(`tar failed with ${String(result.status ?? result.error)}: ${args.join(" ")}`);
  }
}

function makeZip(): string {
  if (!existsSync(BUILT)) {
    throw new Error(`${BUILT} doesn't exist. Run \`npx tauri build\` first.`);
  }
  const version = workspaceVersion(readFileSync("Cargo.toml", "utf8"));
  rmSync(STAGE, { recursive: true, force: true });
  rmSync(OUTPUT, { recursive: true, force: true });
  mkdirSync(join(STAGE, "data"), { recursive: true });
  mkdirSync(OUTPUT, { recursive: true });
  copyFileSync(BUILT, join(STAGE, "tsg.exe"));
  const zip = join(OUTPUT, `Tsugiki_${version}_x64-portable.zip`);
  tar("-a", "-c", "-f", zip, "-C", STAGE, "tsg.exe", "data");
  return zip;
}

/** The folders an installed copy uses (ADR 0014) and Tauri's own, which a portable copy must not create. */
function foldersOutside(): string[] {
  const { LOCALAPPDATA: local, APPDATA: roaming } = process.env;
  return [
    ...(local === undefined ? [] : [join(local, "tsugiki"), join(local, "io.github.tsugiki")]),
    ...(roaming === undefined ? [] : [join(roaming, "tsugiki")]),
  ];
}

function logOf(folder: string): string {
  const log = join(folder, "tsugiki.log");
  return existsSync(log) ? readFileSync(log, "utf8") : "(no tsugiki.log)";
}

function throwIfEnded(app: ChildProcess, data: string): void {
  if (app.exitCode !== null) {
    throw new Error(`tsg.exe ended with ${String(app.exitCode)}. Its log:\n${logOf(data)}`);
  }
}

/** Waits for `tsg.exe` to write its database, then to keep running. */
async function waitUntilRunning(app: ChildProcess, data: string): Promise<void> {
  const database = join(data, "tsugiki.db");
  const deadline = Date.now() + WAIT_FOR_DATABASE_MS;
  while (!existsSync(database)) {
    throwIfEnded(app, data);
    if (Date.now() > deadline) {
      throw new Error(`tsg.exe wrote no database in ${String(WAIT_FOR_DATABASE_MS)} ms: ${logOf(data)}`);
    }
    await wait(250);
  }
  const end = Date.now() + KEEP_RUNNING_MS;
  while (Date.now() < end) {
    throwIfEnded(app, data);
    await wait(250);
  }
}

function stop(app: ChildProcess): void {
  if (app.pid !== undefined && app.exitCode === null) {
    const stopped = spawnSync("taskkill", ["/pid", String(app.pid), "/t", "/f"]);
    if (stopped.status !== 0) {
      console.error(`taskkill couldn't stop tsg.exe (process ${String(app.pid)}), so it's still running.`);
    }
  }
  // Otherwise a copy that wouldn't stop would keep this script waiting until the job's timeout.
  app.unref();
}

async function tryZip(zip: string): Promise<void> {
  rmSync(UNPACKED, { recursive: true, force: true });
  mkdirSync(UNPACKED, { recursive: true });
  tar("-x", "-f", zip, "-C", UNPACKED);
  const data = join(UNPACKED, "data");
  if (!existsSync(data)) {
    throw new Error(`${zip} has no data folder, so it wouldn't be portable.`);
  }
  const before = foldersOutside().filter((folder) => existsSync(folder));

  const app = spawn(join(UNPACKED, "tsg.exe"), [], { stdio: "ignore" });
  const notStarted = new Promise<never>((_, reject) => {
    app.once("error", reject);
  });
  try {
    await Promise.race([waitUntilRunning(app, data), notStarted]);
  } finally {
    stop(app);
  }

  const created = foldersOutside().filter((folder) => existsSync(folder) && !before.includes(folder));
  if (created.length > 0) {
    throw new Error(`The portable copy created ${created.join(" and ")}. Its log:\n${logOf(data)}`);
  }
  console.log(`${zip} starts, and keeps its data in its own folder.`);
}

try {
  const zip = makeZip();
  console.log(zip);
  if (process.argv.includes("--try")) {
    await tryZip(zip);
  }
} catch (cause) {
  console.error(cause instanceof Error ? cause.message : String(cause));
  process.exit(1);
}
