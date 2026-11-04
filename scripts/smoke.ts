// The Windows smoke test (ADR 0009): builds a release and checks its real window, which nothing else can do.
// It runs by hand and before a release. It doesn't gate PRs.
//
// It takes the foreground and presses real keys, so leave the keyboard alone while it runs, and close any
// released Tsugiki first. Keys sent through WebDriver go straight to the page and never reach the engine's
// shortcuts: with those on and the guard gone, they still didn't open Print. Only keys from the OS can show
// the bug.
//
// It reads the page over the Chrome DevTools Protocol, on a port only this test's build opens. We don't use
// WebDriver because an msedgedriver session opens a second window on the primary display (#76).
//
// It needs `gh`, but not your sign-in: it gives Tsugiki a made-up token, and a proxy that decides when
// "GitHub" stops answering, so the status changes when the test says and nothing reaches GitHub.
//
//   npm run smoke
//
// `npm run smoke -- --at 3520,80` opens the window at that place, such as on another display.
import { type ChildProcess, spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer, type AddressInfo, type Socket } from "node:net";
import { join, resolve } from "node:path";
import { setTimeout as wait } from "node:timers/promises";

const DEBUG_PORT = 9333;
// Only this page gets the test's scripts, in case something else already listens on the port.
const APP_ADDRESS = "http://tauri.localhost/";
const SETTLE_MS = 2000;
const CLOSE_MS = 10_000;
// Tsugiki runs `gh auth token` before its first request and gives `gh` 30 s (`TIMEOUT` in live.rs). One run's
// request took 18 s to arrive, cause unknown (#86). This wait starts once the page is ready, so a `gh` that
// times out still fails the check.
const FIRST_REQUEST_MS = 35_000;
const BUILD_CONFIG = ".temp/smoke-config.json";
const TAURI_BROWSER_ARGS = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

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
  run: <T>(expression: string) => Promise<T>;
  press: (...keys: number[]) => void;
  github: FakeGitHub;
  secondsRunning: () => number;
};

type FakeGitHub = {
  port: number;
  /** How many requests are waiting for an answer. */
  held: () => number;
  /** How many requests have arrived since the test started. */
  requests: () => number;
  /** Drops those requests and any later ones, which Tsugiki reads as being offline. */
  cutOff: () => void;
};

/** Listens as Tsugiki's proxy and leaves each request waiting, so Tsugiki stays on "Connecting…". */
async function startFakeGitHub(): Promise<FakeGitHub> {
  const held = new Set<Socket>();
  let cut = false;
  let requests = 0;
  const server = createServer((socket) => {
    requests += 1;
    socket.on("close", () => {
      held.delete(socket);
    });
    socket.on("error", () => {
      // Without a listener, Tsugiki hanging up when it quits would end this script before its clean-up.
    });
    if (cut) {
      socket.destroy();
    } else {
      held.add(socket);
    }
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return {
    port: (server.address() as AddressInfo).port,
    held: () => held.size,
    requests: () => requests,
    cutOff: () => {
      cut = true;
      for (const socket of held) {
        socket.destroy();
      }
      held.clear();
    },
  };
}

type Target = { type: string; url: string; webSocketDebuggerUrl: string };

type Evaluated = {
  error?: unknown;
  result?: { result: { value?: unknown }; exceptionDetails?: unknown };
};

async function findPage(): Promise<string> {
  let seen = "the port never answered";
  for (let tries = 0; tries < 240; tries += 1) {
    try {
      const reply = await fetch(`http://127.0.0.1:${String(DEBUG_PORT)}/json/list`);
      const targets = (await reply.json()) as Target[];
      const page = targets.find((t) => t.type === "page" && t.url.startsWith(APP_ADDRESS));
      if (page !== undefined) {
        return page.webSocketDebuggerUrl;
      }
      seen = `the port lists ${JSON.stringify(targets.map((t) => `${t.type} ${t.url}`))}`;
    } catch {
      // The port isn't open yet, and running out of tries is the failure to report.
      seen = "the port never answered";
    }
    await wait(250);
  }
  throw new Error(`Tsugiki's page didn't appear, and ${seen}. Is a released Tsugiki open?`);
}

async function connect(address: string): Promise<WebSocket> {
  const socket = new WebSocket(address);
  await new Promise((opened, failed) => {
    socket.addEventListener("open", opened, { once: true });
    socket.addEventListener(
      "error",
      () => {
        failed(new Error("The connection to the page didn't open."));
      },
      { once: true },
    );
  });
  return socket;
}

/** Runs one expression at a time. The test turns on no protocol events, so the next message is the reply. */
function evaluate(socket: WebSocket, expression: string): Promise<unknown> {
  return new Promise((answered, failed) => {
    // Sending on a closed socket does nothing, and no reply or `close` event would ever end this wait.
    if (socket.readyState !== WebSocket.OPEN) {
      failed(new Error(`The page's connection had closed before: ${expression}`));
      return;
    }
    const replied = new AbortController();
    socket.addEventListener(
      "close",
      () => {
        failed(new Error(`The page's connection closed during: ${expression}`));
      },
      { once: true, signal: replied.signal },
    );
    socket.addEventListener(
      "message",
      (message: MessageEvent<string>) => {
        replied.abort();
        const reply = JSON.parse(message.data) as Evaluated;
        if (reply.result === undefined || reply.result.exceptionDetails !== undefined) {
          failed(new Error(`${expression}: ${message.data}`));
          return;
        }
        answered(reply.result.result.value);
      },
      { once: true },
    );
    const params = { expression, returnByValue: true };
    socket.send(JSON.stringify({ id: 1, method: "Runtime.evaluate", params }));
  });
}

function sessionFor(
  socket: WebSocket,
  application: string,
  github: FakeGitHub,
  secondsRunning: () => number,
): Session {
  return {
    github,
    secondsRunning,
    run: async <T>(expression: string) => (await evaluate(socket, expression)) as T,
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

const STATUS_BAR_TEXT = "document.querySelector('[role=status]')?.textContent.trim() ?? ''";

/** Asks every 250 ms, for five seconds unless told otherwise, and says whether the answer was ever yes. */
async function until(condition: () => boolean | Promise<boolean>, patienceMs = 5000): Promise<boolean> {
  for (let tries = 0; tries < patienceMs / 250; tries += 1) {
    if (await condition()) {
      return true;
    }
    await wait(250);
  }
  return false;
}

/** Waits for the status bar to show `text`, and returns what it shows instead if it doesn't. */
async function statusBarShows(run: Session["run"], text: string): Promise<string | undefined> {
  let shown = "";
  const showed = await until(async () => {
    shown = await run<string>(STATUS_BAR_TEXT);
    return shown.includes(text);
  });
  return showed ? undefined : `the status bar says "${shown}", and should say "${text}"`;
}

/** Each check returns what went wrong, or nothing. */
const CHECKS: Record<string, (session: Session) => Promise<string | undefined>> = {
  // First, because it needs Tsugiki's first request to GitHub still waiting. Without the event the page has
  // no other way to learn this: it only asks `status()` when it loads, and the settling gives it time to.
  "a status change reaches the open window": async ({ run, github, secondsRunning }) => {
    try {
      if (!(await until(() => github.held() > 0, FIRST_REQUEST_MS))) {
        const shown = await run<string>(STATUS_BAR_TEXT);
        const running = String(secondsRunning());
        const requests = String(github.requests());
        return `after ${running} s, no request is waiting at the test's proxy, which has seen ${requests}, and the status bar says "${shown}"`;
      }
      console.log(
        `Tsugiki's first request is waiting at the proxy, ${String(secondsRunning())} s after it started`,
      );
      const problem = await statusBarShows(run, "Connecting…");
      if (problem !== undefined) {
        return problem;
      }
      github.cutOff();
      return await statusBarShows(run, "Offline");
    } finally {
      // A request left waiting would fail later checks too, and blame the database when the window closes.
      github.cutOff();
    }
  },

  // Tsugiki announced this status before the new page loaded, so only `status()` can give it to the page.
  "a window that loads later gets the current status": async ({ run }) => {
    await run("window.smokeOldPage = true; setTimeout(() => location.reload(), 0)");
    const reloaded = await until(async () => {
      try {
        const newPage = "window.smokeOldPage === undefined && document.readyState === 'complete'";
        return await run<boolean>(newPage);
      } catch {
        // The page is between documents, and running out of tries is the failure to report.
        return false;
      }
    });
    return reloaded ? statusBarShows(run, "Offline") : "the page didn't reload";
  },

  // Before the other key checks, because they would also pass if no key reached the window at all.
  "Ctrl+Shift+P opens the palette, and Esc closes it": async ({ run, press }) => {
    const paletteIsOpen = "document.querySelector('[role=dialog]') !== null";
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
    const prints = await run<number>("window.smokePrints");
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
    const same = await run<boolean>("window.smokeSameLoad ?? false");
    return same === true ? undefined : "the page reloaded";
  },
};

/** Prints the result, and returns how many failures to count. */
function report(name: string, problem: string | undefined): number {
  console.log(problem === undefined ? `ok    ${name}` : `FAIL  ${name}: ${problem}`);
  return problem === undefined ? 0 : 1;
}

async function runChecks(
  application: string,
  github: FakeGitHub,
  secondsRunning: () => number,
): Promise<number> {
  const socket = await connect(await findPage());
  console.log(`The page answered ${String(secondsRunning())} s after Tsugiki started`);
  const session = sessionFor(socket, application, github, secondsRunning);
  let failures = 0;
  try {
    await until(async () => (await session.run<string>("document.readyState")) === "complete");
    await wait(SETTLE_MS);
    for (const [name, check] of Object.entries(CHECKS)) {
      failures += report(name, await check(session));
    }
  } finally {
    socket.close();
  }
  return failures;
}

/** SQLite removes the two files when the session closes the database, so they show that it didn't. */
function whatClosingLeft(folder: string): string | undefined {
  if (!existsSync(join(folder, "tsugiki.db"))) {
    return "there's no database to check";
  }
  const left = ["tsugiki.db-wal", "tsugiki.db-shm"].filter((name) => existsSync(join(folder, name)));
  return left.length === 0 ? undefined : `it left ${left.join(" and ")}`;
}

/** A portable copy keeps everything in its folder (ADR 0014), and the webview's data is the part Tauri wouldn't. */
function whereTheWebviewWrote(folder: string): string | undefined {
  return existsSync(join(folder, "webview")) ? undefined : `${folder} has no webview folder`;
}

/** WebView2's processes outlive `tsg.exe` by some seconds, and hold files in `webview` until they end. */
async function removeWhenFree(folder: string): Promise<void> {
  const deadline = Date.now() + 60_000;
  for (;;) {
    try {
      rmSync(folder, { recursive: true });
      return;
    } catch (cause) {
      if (Date.now() > deadline) {
        throw cause;
      }
      await wait(500);
    }
  }
}

/** Asks the window to close, as a user would, so the app shuts down its store before the test deletes it. */
async function close(app: ChildProcess): Promise<void> {
  // A process killed by a signal keeps a null `exitCode`, and one that never started has no `pid`.
  if (app.pid === undefined || app.exitCode !== null || app.signalCode !== null) {
    return;
  }
  spawnSync("taskkill", ["/PID", String(app.pid)], { windowsHide: true });
  try {
    await once(app, "exit", { signal: AbortSignal.timeout(CLOSE_MS) });
  } catch {
    console.error(`Tsugiki didn't close within ${String(CLOSE_MS)} ms, so the test killed it.`);
    app.kill();
    await once(app, "exit");
  }
}

function exitWith(message: string): never {
  console.error(message);
  process.exit(1);
}

/** Where `--at x,y` puts the window, if it's on the command line. */
function windowPlace(): { x: number; y: number } | undefined {
  const [flag, place, ...others] = process.argv.slice(2);
  if (flag === undefined) {
    return undefined;
  }
  const numbers = flag === "--at" && others.length === 0 ? /^(-?\d+),(-?\d+)$/.exec(place ?? "") : null;
  if (numbers === null) {
    exitWith("The only option is --at x,y, in whole numbers.");
  }
  return { x: Number(numbers[1]), y: Number(numbers[2]) };
}

/**
 * Writes the app's window settings with the debugging port on, for this build only, and returns the path.
 * WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS needs no build setting, but WebView2 ignored it on a GitHub runner.
 * The setting replaces Tauri's default arguments, so without the app's own this repeats those.
 */
function writeBuildConfig(): string {
  const appConfig = JSON.parse(readFileSync("crates/app/tauri.conf.json", "utf8")) as {
    app: { windows: ({ additionalBrowserArgs?: string } & Record<string, unknown>)[] };
  };
  const windows = appConfig.app.windows.map((window) => ({
    ...window,
    ...windowPlace(),
    additionalBrowserArgs: `--remote-debugging-port=${String(DEBUG_PORT)} ${window.additionalBrowserArgs ?? TAURI_BROWSER_ARGS}`,
  }));
  mkdirSync(".temp", { recursive: true });
  writeFileSync(BUILD_CONFIG, JSON.stringify({ app: { windows } }));
  return BUILD_CONFIG;
}

if (process.platform !== "win32") {
  exitWith("The smoke test runs on Windows: WKWebView doesn't speak the DevTools Protocol (ADR 0009).");
}
// A folder here makes the release portable, so the test never touches the real data (ADR 0014).
const data = resolve("target/release/data");
if (existsSync(data)) {
  exitWith(`${data} already exists, and the test deletes it afterwards, so look at it first.`);
}

// This build opens an unguarded debugging port, and without `data` it would use the real data and sign-in, so
// it mustn't outlive the test. Stopping the test with Ctrl+C skips this: delete the file by hand then.
const application = resolve("target/release/tsg.exe");
process.on("exit", () => {
  rmSync(application, { force: true, maxRetries: 10, retryDelay: 500 });
});

const buildCommand = `npx tauri build --no-bundle --config ${writeBuildConfig()} -- --locked`;
const build = spawnSync(buildCommand, { stdio: "inherit", shell: true, windowsHide: true });
if (build.status !== 0) {
  exitWith("The release didn't build.");
}

const github = await startFakeGitHub();
mkdirSync(data, { recursive: true });
const started = Date.now();
const secondsRunning = (): number => Math.round((Date.now() - started) / 1000);
const app = spawn(application, [], {
  stdio: "ignore",
  env: {
    ...process.env,
    // `gh auth token` prints this instead of reading your sign-in.
    GH_TOKEN: "smoke-test-not-a-token",
    // The first variable the HTTP client looks at, so a proxy of your own can't take the request.
    ALL_PROXY: `http://127.0.0.1:${String(github.port)}`,
    NO_PROXY: "",
  },
});
// Without a listener a failed start would end the process before the clean-up below, and leave `data`.
app.on("error", (cause) => {
  console.error(`Tsugiki didn't start: ${cause.message}`);
});
// A check that throws counts as a failure too.
let failures = 1;
try {
  failures = await runChecks(application, github, secondsRunning);
  await close(app);
  failures += report("closing the window closes the database", whatClosingLeft(data));
  failures += report("the webview keeps its data in the portable folder", whereTheWebviewWrote(data));
} finally {
  if (failures > 0) {
    // The clean-up below deletes the log, which may say why.
    const log = join(data, "tsugiki.log");
    console.error(`Tsugiki's exit code so far: ${String(app.exitCode)}`);
    console.error(existsSync(log) ? readFileSync(log, "utf8") : "Tsugiki wrote no log.");
  }
  await close(app);
  await removeWhenFree(data);
}
process.exit(failures === 0 ? 0 : 1);
