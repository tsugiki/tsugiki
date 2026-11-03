// The Windows smoke test (ADR 0009): builds a release and checks its real window, which nothing else can do.
// It runs by hand and before a release. It doesn't gate PRs.
//
// It takes the foreground and presses real keys, so leave the keyboard alone while it runs, and close any
// released Tsugiki first. Keys sent through WebDriver go straight to the page and never reach the engine's
// shortcuts: with those on and the guard gone, they still didn't open Print. Only keys from the OS can show
// the bug.
//
// It reads the page over the Chrome DevTools Protocol, on a port that WebView2 opens for this run. We don't
// use WebDriver because an msedgedriver session opens a second window on the primary display (#76).
//
//   npm run smoke
//
// Anything after `--` goes to `tauri build`, such as `--config second-display.json`.
import { type ChildProcess, spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { existsSync, mkdirSync, rmSync } from "node:fs";
import { resolve } from "node:path";
import { setTimeout as wait } from "node:timers/promises";

const DEBUG_PORT = 9333;
// Only this page gets the test's scripts, in case something else already listens on the port.
const APP_ADDRESS = "http://tauri.localhost/";
const SETTLE_MS = 2000;
const CLOSE_MS = 10_000;

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
};

type Target = { type: string; url: string; webSocketDebuggerUrl: string };

type Evaluated = {
  error?: unknown;
  result?: { result: { value?: unknown }; exceptionDetails?: unknown };
};

async function findPage(): Promise<string> {
  for (let tries = 0; tries < 80; tries += 1) {
    try {
      const reply = await fetch(`http://127.0.0.1:${String(DEBUG_PORT)}/json/list`);
      const targets = (await reply.json()) as Target[];
      const page = targets.find((t) => t.type === "page" && t.url.startsWith(APP_ADDRESS));
      if (page !== undefined) {
        return page.webSocketDebuggerUrl;
      }
    } catch {
      // The port isn't open yet, and running out of tries is the failure to report.
    }
    await wait(250);
  }
  throw new Error(`Tsugiki's page didn't appear on port ${String(DEBUG_PORT)}. Is a released Tsugiki open?`);
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
    socket.addEventListener(
      "close",
      () => {
        failed(new Error(`The page's connection closed during: ${expression}`));
      },
      { once: true },
    );
    socket.addEventListener(
      "message",
      (message: MessageEvent<string>) => {
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

function sessionFor(socket: WebSocket, application: string): Session {
  return {
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

/** Each check returns what went wrong, or nothing. */
const CHECKS: Record<string, (session: Session) => Promise<string | undefined>> = {
  // First, because the checks after it would also pass if no key reached the window at all.
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

async function runChecks(application: string): Promise<number> {
  const socket = await connect(await findPage());
  const session = sessionFor(socket, application);
  let failures = 0;
  try {
    await wait(SETTLE_MS);
    for (const [name, check] of Object.entries(CHECKS)) {
      const problem = await check(session);
      console.log(problem === undefined ? `ok    ${name}` : `FAIL  ${name}: ${problem}`);
      failures += problem === undefined ? 0 : 1;
    }
  } finally {
    socket.close();
  }
  return failures;
}

/** Asks the window to close, as a user would, so the app shuts down its store before the test deletes it. */
async function close(app: ChildProcess): Promise<void> {
  if (app.exitCode !== null) {
    return;
  }
  spawnSync("taskkill", ["/PID", String(app.pid)], { windowsHide: true });
  try {
    await once(app, "exit", { signal: AbortSignal.timeout(CLOSE_MS) });
  } catch {
    console.error(`Tsugiki didn't close within ${String(CLOSE_MS)} ms, so the test killed it.`);
    app.kill();
  }
}

function exitWith(message: string): never {
  console.error(message);
  process.exit(1);
}

if (process.platform !== "win32") {
  exitWith("The smoke test runs on Windows: WKWebView doesn't speak the DevTools Protocol (ADR 0009).");
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
const application = resolve("target/release/tsg.exe");
const app = spawn(application, [], {
  stdio: "ignore",
  env: {
    ...process.env,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${String(DEBUG_PORT)}`,
  },
});
// Without a listener a failed start would end the process before the clean-up below, and leave `data`.
app.on("error", (cause) => {
  console.error(`Tsugiki didn't start: ${cause.message}`);
});
let failures: number;
try {
  failures = await runChecks(application);
} finally {
  await close(app);
  rmSync(data, { recursive: true, maxRetries: 10, retryDelay: 500 });
}
process.exit(failures === 0 ? 0 : 1);
