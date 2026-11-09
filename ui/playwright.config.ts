import { defineConfig, devices } from "@playwright/test";

const port = 4183;

// The built UI, with Tauri's IPC mocked, in Chromium and in WebKit, which is close to WKWebView (ADR 0009).
// Serves build/, the files Tauri ships, not Vite's preview of them.
export default defineConfig({
  testDir: "e2e",
  // A failed test's traces, in the ignored .temp beside the other test output.
  outputDir: "../.temp/playwright-results",
  forbidOnly: !!process.env["CI"],
  reporter: process.env["CI"] ? "github" : "list",
  use: { baseURL: `http://localhost:${port}`, trace: "retain-on-failure" },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
    { name: "webkit", use: { ...devices["Desktop Safari"] } },
  ],
  webServer: {
    command: `npm run build && sirv build --single --port ${port}`,
    port,
    timeout: 300_000,
  },
});
