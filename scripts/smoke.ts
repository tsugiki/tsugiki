// The Windows smoke test (ADR 0009): builds a release and drives its real window through tauri-driver, which
// nothing else can do. It runs by hand and before a release. It doesn't gate PRs.
//
// It takes the foreground and presses real keys, so leave the keyboard alone while it runs, and close any
// released Tsugiki first. Keys sent through WebDriver go straight to the page and never reach the engine's
// shortcuts: with those on and the guard gone, they still didn't open Print. Only keys from the OS can show
// the bug.
//
//   cargo install tauri-driver --version '^2' --locked
//   download msedgedriver for your WebView2 runtime into .temp/edgedriver, or set MSEDGEDRIVER to its path
//   npm run smoke
//
// Anything after `--` goes to `tauri build`, such as `--config second-display.json`.
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, rmSync } from "node:fs";
import { resolve } from "node:path";
import { setTimeout as wait } from "node:timers/promises";

const DRIVER = "http://127.0.0.1:4444";
const SETTLE_MS = 2000;

// Windows' virtual-key codes, which name physical keys whatever the keyboard layout is.
const CONTROL = 0x11;
const SHIFT = 0x10;
const ESCAPE = 0x1b;
const F5 = 0x74;
const P = 0x50;

// Brings the window forward, checks it got there, and presses the keys, last one first on the way up. Windows
// refuses the foreground to a process that hasn't had input, so this presses F24, which nothing uses, first.
// It finds the window by title, because `MainWindowHandle` can pick the single-instance plugin's invisible
// window.
const PRESS_KEYS = `
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class Input {
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("user32.dll")] public static extern uint MapVirtualKey(uint code, uint type);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string kind, string title);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
  public static uint ForegroundProcess() {
    uint process;
    GetWindowThreadProcessId(GetForegroundWindow(), out process);
    return process;
  }
  public static void Send(byte vk, uint flags) {
    keybd_event(vk, (byte)MapVirtualKey(vk, 0), flags, UIntPtr.Zero);
  }
}
"@
$tsg = Get-Process tsg | Where-Object { $_.Path -eq $env:SMOKE_EXE } | Select-Object -First 1
if (-not $tsg) { Write-Error 'tsg is not running'; exit 3 }
$window = [Input]::FindWindow([NullString]::Value, 'Tsugiki')
foreach ($try in 1..10) {
  [Input]::Send(0x87, 0)
  [Input]::Send(0x87, 2)
  [void][Input]::SetForegroundWindow($window)
  Start-Sleep -Milliseconds 300
  if ([Input]::ForegroundProcess() -eq $tsg.Id) { break }
}
if ([Input]::ForegroundProcess() -ne $tsg.Id) { Write-Error 'tsg is not in the foreground'; exit 4 }
$keys = $env:SMOKE_KEYS -split ',' | ForEach-Object { [byte]$_ }
foreach ($key in $keys) { [Input]::Send($key, 0) }
[array]::Reverse($keys)
foreach ($key in $keys) { [Input]::Send($key, 2) }
`;

type Session = {
  run: <T>(script: string) => Promise<T>;
  press: (...keys: number[]) => void;
};

async function webdriver(path: string, method: string, body?: unknown): Promise<unknown> {
  const options: RequestInit = { method, headers: { "content-type": "application/json" } };
  if (body !== undefined) {
    options.body = JSON.stringify(body);
  }
  const reply = await fetch(`${DRIVER}${path}`, options);
  const { value } = (await reply.json()) as { value: unknown };
  if (!reply.ok) {
    throw new Error(`${method} ${path}: ${JSON.stringify(value)}`);
  }
  return value;
}

async function waitForDriver(): Promise<void> {
  for (let tries = 0; tries < 40; tries += 1) {
    try {
      await fetch(`${DRIVER}/status`);
      return;
    } catch {
      // The driver isn't listening yet, and running out of tries is the failure to report.
      await wait(250);
    }
  }
  throw new Error("tauri-driver didn't start listening");
}

function sessionFor(id: string, application: string): Session {
  return {
    run: async <T>(script: string) => {
      const answer = await webdriver(`/session/${id}/execute/sync`, "POST", { script, args: [] });
      return answer as T;
    },
    press: (...keys) => {
      const env = { ...process.env, SMOKE_EXE: application, SMOKE_KEYS: keys.join(",") };
      const pressed = spawnSync("powershell", ["-NoProfile", "-Command", PRESS_KEYS], {
        env,
        encoding: "utf8",
        windowsHide: true,
      });
      if (pressed.status !== 0) {
        throw new Error(`The real keys didn't go in: ${pressed.stderr}`);
      }
    },
  };
}

/** Each check returns what went wrong, or nothing. */
const CHECKS: Record<string, (session: Session) => Promise<string | undefined>> = {
  // First, because the checks after it would also pass if no key reached the window at all.
  "Ctrl+Shift+P opens the palette, and Esc closes it": async ({ run, press }) => {
    const paletteIsOpen = "return document.querySelector('[role=dialog]') !== null;";
    press(CONTROL, SHIFT, P);
    await wait(SETTLE_MS);
    if (!(await run<boolean>(paletteIsOpen))) {
      return "the palette didn't open";
    }
    press(ESCAPE);
    await wait(SETTLE_MS);
    return (await run<boolean>(paletteIsOpen)) ? "the palette didn't close" : undefined;
  },

  "Ctrl+P never opens Print": async ({ run, press }) => {
    await run("window.smokePrints = 0; addEventListener('beforeprint', () => { window.smokePrints += 1; });");
    press(CONTROL, P);
    await wait(SETTLE_MS);
    const prints = await run<number>("return window.smokePrints;");
    if (prints > 0) {
      press(ESCAPE);
      await wait(SETTLE_MS);
      return `Print opened ${String(prints)} time(s)`;
    }
    return undefined;
  },

  // The page's guard doesn't own F5, so only the engine's switch stops it.
  "the engine's shortcuts are off: F5 doesn't reload": async ({ run, press }) => {
    await run("window.smokeSameLoad = true;");
    press(F5);
    await wait(SETTLE_MS);
    const same = await run<boolean | null>("return window.smokeSameLoad ?? false;");
    return same === true ? undefined : "the page reloaded";
  },
};

async function runChecks(application: string): Promise<number> {
  await waitForDriver();
  const created = (await webdriver("/session", "POST", {
    capabilities: { alwaysMatch: { "tauri:options": { application } } },
  })) as { sessionId: string };
  const session = sessionFor(created.sessionId, application);
  let failures = 0;
  try {
    await wait(SETTLE_MS);
    for (const [name, check] of Object.entries(CHECKS)) {
      const problem = await check(session);
      console.log(problem === undefined ? `ok    ${name}` : `FAIL  ${name}: ${problem}`);
      failures += problem === undefined ? 0 : 1;
    }
  } finally {
    await webdriver(`/session/${created.sessionId}`, "DELETE");
  }
  return failures;
}

function exitWith(message: string): never {
  console.error(message);
  process.exit(1);
}

if (process.platform !== "win32") {
  exitWith("The smoke test runs on Windows: tauri-driver has no WebDriver for WKWebView (ADR 0009).");
}
const edgeDriver = resolve(process.env["MSEDGEDRIVER"] ?? ".temp/edgedriver/msedgedriver.exe");
if (!existsSync(edgeDriver)) {
  exitWith(`There's no msedgedriver at ${edgeDriver}. The top of scripts/smoke.ts says how to get one.`);
}
// A folder here makes the release portable, so the test never touches the real data (ADR 0014).
const data = resolve("target/release/data");
if (existsSync(data)) {
  exitWith(`${data} already exists, and the test deletes it afterwards, so look at it first.`);
}

const buildCommand = ["npx tauri build --no-bundle", ...process.argv.slice(2)].join(" ");
const build = spawnSync(buildCommand, { stdio: "inherit", shell: true, windowsHide: true });
if (build.status !== 0) {
  exitWith("The release didn't build.");
}

mkdirSync(data, { recursive: true });
const driver = spawn("tauri-driver", ["--native-driver", edgeDriver], {
  stdio: "ignore",
  windowsHide: true,
});
// Without a listener a missing tauri-driver would end the process before the clean-up below, and leave `data`.
driver.on("error", (cause) => {
  console.error(`tauri-driver didn't start: ${cause.message}`);
});
let failures: number;
try {
  failures = await runChecks(resolve("target/release/tsg.exe"));
} finally {
  driver.kill();
  rmSync(data, { recursive: true, maxRetries: 10, retryDelay: 500 });
}
process.exit(failures === 0 ? 0 : 1);
