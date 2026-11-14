import { fireEvent, render, screen } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { openLink } from "$lib/state/browser";
import Settings from "./Settings.svelte";

const core = vi.hoisted(() => ({ tokenSource: vi.fn() }));

vi.mock("$lib/data/bindings", () => ({ commands: core }));
vi.mock("$lib/state/browser", () => ({ openLink: vi.fn() }));

const README = "https://github.com/tsugiki/tsugiki/blob/v0.3.1/crates/token/README.md";
const about = { name: "Tsugiki", version: "0.3.1", build: "release", source_ref: "v0.3.1" } as const;
const online = { state: "online", login: "octocat" } as const;
/** The dialog's props, with the status online and this release's About. */
const props = (onclose = () => {}) => ({ status: online, about, onclose });
const dialog = () => screen.getByRole("dialog", { name: "Settings" });
const page = () => screen.getByRole("region", { name: "Account" });

beforeEach(() => {
  vi.clearAllMocks();
  core.tokenSource.mockResolvedValue("gh");
});

describe("the Settings dialog", () => {
  it("lists Account in its left column, the page it shows", () => {
    render(Settings, props());
    const pages = screen.getByRole("navigation", { name: "Settings pages" });
    const entries = Array.from(pages.querySelectorAll("button"));
    expect(entries.map((entry) => entry.textContent)).toEqual(["Account"]);
    expect(entries[0]?.getAttribute("aria-current")).toBe("page");
    expect(page()).toBeTruthy();
  });

  it("takes the focus, and gives it back when it goes", () => {
    const before = document.createElement("button");
    document.body.append(before);
    before.focus();
    const { unmount } = render(Settings, props());
    expect(document.activeElement).toBe(dialog());

    unmount();
    expect(document.activeElement).toBe(before);
    before.remove();
  });

  it("keeps Tab inside, from its last control round to ✕", async () => {
    render(Settings, props());
    screen.getByRole("link", { name: "README" }).focus();
    expect(await fireEvent.keyDown(document.activeElement as Element, { key: "Tab" })).toBe(false);
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Close" }));
  });

  it("closes on Esc, on ✕ and on a press outside, but not on a press inside", async () => {
    const onclose = vi.fn();
    render(Settings, props(onclose));

    await fireEvent.keyDown(window, { key: "Enter" });
    await fireEvent.mouseDown(dialog());
    expect(onclose).not.toHaveBeenCalled();

    await fireEvent.keyDown(window, { key: "Escape" });
    expect(onclose).toHaveBeenCalledTimes(1);
    await fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(onclose).toHaveBeenCalledTimes(2);
    // A browser would move the focus to the body after the dialog gave it back, if not stopped.
    expect(await fireEvent.mouseDown(screen.getByTestId("settings-scrim"))).toBe(false);
    expect(onclose).toHaveBeenCalledTimes(3);
  });
});

describe("Settings › Account", () => {
  /** The value beside `label`. */
  const value = (label: string) =>
    screen.getByText(label, { selector: "dt" }).nextElementSibling?.textContent.replace(/\s+/g, " ").trim();

  it("shows who is signed in and that the token comes from gh", async () => {
    render(Settings, props());
    expect(value("Signed in as")).toBe("octocat");
    expect(await screen.findByText("gh", { selector: "b" })).toBeTruthy();
    expect(value("Token from")).toBe("gh Your token comes from the GitHub CLI (gh auth token).");
    expect(core.tokenSource).toHaveBeenCalledOnce();
  });

  it("says when the login is last time's, and why it names nobody", async () => {
    const { rerender } = render(Settings, {
      ...props(),
      status: { state: "offline_as", last_login: "octocat" },
    });
    expect(value("Signed in as")).toBe("octocat Signed in last time. GitHub can't be reached now.");

    await rerender({ ...props(), status: undefined });
    expect(value("Signed in as")).toBe("Not known yet: GitHub hasn't answered.");
  });

  it("says nothing of the token's source until the core answers, and says why if it can't", async () => {
    core.tokenSource.mockRejectedValue(new Error("no IPC outside Tauri"));
    render(Settings, props());
    expect(value("Token from")).toBe("");
    expect(await screen.findByText("Tsugiki can't say: Error: no IPC outside Tauri")).toBeTruthy();
  });

  it("says what Tsugiki does with the token", () => {
    render(Settings, props());
    expect(screen.getByRole("heading", { name: "What Tsugiki does with the token" })).toBeTruthy();
    const said = Array.from(page().querySelectorAll("li")).map((line) =>
      line.textContent.replace(/\s+/g, " ").trim(),
    );
    expect(said).toEqual([
      "Only Tsugiki's core, written in Rust, holds it. The window shows PRs and comments that anyone can " +
        "write, so it never gets the token and can't reach the network.",
      "It goes only to api.github.com, with Tsugiki's requests to GitHub's API.",
      "It is never written to the settings files, the database or the log.",
    ]);
  });

  it("opens the token crate's README at this release's tag in the browser, on a click or a middle click", async () => {
    vi.mocked(openLink).mockResolvedValue(undefined);
    render(Settings, props());
    const link = screen.getByRole("link", { name: "README" });
    expect(link.getAttribute("href")).toBe(README);

    // The webview would follow the link itself, or open a window for it, if not stopped.
    expect(await fireEvent.click(link)).toBe(false);
    const auxclick = (button: number) =>
      fireEvent(link, new MouseEvent("auxclick", { button, bubbles: true, cancelable: true }));
    expect(await auxclick(1)).toBe(false);
    expect(openLink).toHaveBeenCalledTimes(2);
    expect(openLink).toHaveBeenCalledWith(README);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("leaves the right button and the mouse's back and forward buttons alone on the README", async () => {
    render(Settings, props());
    const link = screen.getByRole("link", { name: "README" });
    for (const button of [2, 3, 4]) {
      await fireEvent(link, new MouseEvent("auxclick", { button, bubbles: true, cancelable: true }));
    }
    expect(openLink).not.toHaveBeenCalled();
  });

  it("says why the browser didn't open", async () => {
    vi.mocked(openLink).mockResolvedValue("Error: no browser");
    render(Settings, props());
    await fireEvent.click(screen.getByRole("link", { name: "README" }));
    expect((await screen.findByRole("alert")).textContent).toBe("Error: no browser");
  });
});
