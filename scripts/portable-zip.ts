// Zips the Windows portable copy from what `npx tauri build --no-bundle` left in target/release: `tsg.exe`,
// and an empty `data` folder that makes it portable (ADR 0014).
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

function powershell(command: string): void {
  const result = spawnSync("pwsh", ["-NoProfile", "-NonInteractive", "-Command", command], {
    stdio: "inherit",
  });
  if (result.status !== 0) {
    throw new Error(`pwsh failed with ${String(result.status ?? result.error)}: ${command}`);
  }
}

function makeZip(): string {
  if (!existsSync(BUILT)) {
    throw new Error(`${BUILT} doesn't exist. Run \`npx tauri build --no-bundle\` first.`);
  }
  const version = workspaceVersion(readFileSync("Cargo.toml", "utf8"));
  rmSync(STAGE, { recursive: true, force: true });
  mkdirSync(join(STAGE, "data"), { recursive: true });
  copyFileSync(BUILT, join(STAGE, "tsg.exe"));
  mkdirSync(OUTPUT, { recursive: true });
  const zip = join(OUTPUT, `Tsugiki_${version}_x64-portable.zip`);
  rmSync(zip, { force: true });
  // Not Compress-Archive, which leaves out an empty folder.
  powershell(`[IO.Compression.ZipFile]::CreateFromDirectory('${STAGE}', '${zip}')`);
  return zip;
}

/** The folders an installed copy uses (ADR 0014), which a portable copy must not create. */
function installedFolders(): string[] {
  return [process.env.LOCALAPPDATA, process.env.APPDATA].flatMap((root) =>
    root === undefined ? [] : [join(root, "tsugiki")],
  );
}

function logOf(folder: string): string {
  const log = join(folder, "tsugiki.log");
  return existsSync(log) ? readFileSync(log, "utf8") : "(no tsugiki.log)";
}

/** Waits for `tsg.exe` to write its database, which shows that it started and where it keeps its data. */
async function waitForDatabase(app: ChildProcess, data: string): Promise<void> {
  const database = join(data, "tsugiki.db");
  const deadline = Date.now() + WAIT_FOR_DATABASE_MS;
  while (!existsSync(database)) {
    if (app.exitCode !== null) {
      throw new Error(`tsg.exe ended with ${String(app.exitCode)}. Its log:\n${logOf(data)}`);
    }
    if (Date.now() > deadline) {
      throw new Error(`tsg.exe wrote no database in ${String(WAIT_FOR_DATABASE_MS)} ms: ${logOf(data)}`);
    }
    await wait(250);
  }
}

async function tryZip(zip: string): Promise<void> {
  rmSync(UNPACKED, { recursive: true, force: true });
  powershell(`Expand-Archive -Path '${zip}' -DestinationPath '${UNPACKED}'`);
  const data = join(UNPACKED, "data");
  if (!existsSync(data)) {
    throw new Error(`${zip} has no data folder, so it wouldn't be portable.`);
  }
  const before = installedFolders().filter((folder) => existsSync(folder));

  const app = spawn(join(UNPACKED, "tsg.exe"), [], { stdio: "ignore" });
  const notStarted = new Promise<never>((_, reject) => {
    app.once("error", reject);
  });
  try {
    await Promise.race([waitForDatabase(app, data), notStarted]);
  } finally {
    if (app.pid !== undefined) {
      spawnSync("taskkill", ["/pid", String(app.pid), "/t", "/f"]);
    }
  }

  const created = installedFolders().filter((folder) => existsSync(folder) && !before.includes(folder));
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
