import { describe, expect, it, vi } from "vitest";
import { commands } from "$lib/data/bindings";
import { OCTOCAT_ONLINE, statusOf } from "$lib/testing/fixtures";
import { AccountState, signedIn, tokenReadme } from "./account.svelte";

vi.mock("$lib/data/bindings", () => ({ commands: { tokenSource: vi.fn() } }));

describe("signedIn", () => {
  it("names the login GitHub confirmed", () => {
    expect(signedIn(OCTOCAT_ONLINE)).toEqual({ login: "octocat" });
  });

  it("names last time's login, and says so, while GitHub hasn't answered or can't be reached", () => {
    expect(signedIn(statusOf({ state: "connecting_as", last_login: "octocat" }))).toEqual({
      login: "octocat",
      note: "Signed in last time. GitHub hasn't answered yet.",
    });
    expect(signedIn(statusOf({ state: "offline_as", last_login: "octocat" }))).toEqual({
      login: "octocat",
      note: "Signed in last time. GitHub can't be reached now.",
    });
    expect(signedIn(statusOf({ state: "reconnecting_as", last_login: "octocat" }))).toEqual({
      login: "octocat",
      note: "Signed in last time. GitHub hasn't answered yet.",
    });
  });

  it("says why it names nobody", () => {
    const notYet = { note: "Not known yet: GitHub hasn't answered." };
    expect(signedIn(undefined)).toEqual(notYet);
    expect(signedIn(statusOf({ state: "connecting" }))).toEqual(notYet);
    expect(signedIn(statusOf({ state: "reconnecting" }))).toEqual(notYet);
    expect(signedIn(statusOf({ state: "offline" }))).toEqual({ note: "Not known: GitHub can't be reached." });
    expect(signedIn(statusOf({ state: "unconfirmed" }))).toEqual({
      note: "Not known: GitHub's last answer didn't say.",
    });
    expect(signedIn(statusOf({ state: "stopped" }))).toEqual({
      note: "Not known: Tsugiki stopped asking GitHub. The status bar says why.",
    });
  });

  it("names who was signed in last time when GitHub's last answer didn't say who is now", () => {
    expect(signedIn(statusOf({ state: "unconfirmed_as", last_login: "octocat" }))).toEqual({
      login: "octocat",
      note: "Signed in last time. GitHub's last answer didn't say who is now.",
    });
  });

  it("says what is wrong with the token in the core's words", () => {
    expect(signedIn(statusOf({ state: "refused", message: "Run gh auth login." }))).toEqual({
      note: "Run gh auth login.",
    });
    expect(signedIn(statusOf({ state: "no_token", message: "Install gh." }))).toEqual({
      note: "Install gh.",
    });
  });

  it("names the login whatever problem shows beside it", () => {
    const problem = { kind: "internal", message: "Tsugiki can't read its database." } as const;
    expect(signedIn({ ...OCTOCAT_ONLINE, problem })).toEqual({ login: "octocat" });
  });
});

describe("tokenReadme", () => {
  it("links the token crate's README in this build's source, or on main until the core says", () => {
    const about = { name: "Tsugiki", version: "0.3.1", build: "release", source_ref: "v0.3.1" } as const;
    expect(tokenReadme(about)).toBe("https://github.com/tsugiki/tsugiki/blob/v0.3.1/crates/token/README.md");
    expect(tokenReadme(undefined)).toBe(
      "https://github.com/tsugiki/tsugiki/blob/main/crates/token/README.md",
    );
  });
});

describe("AccountState", () => {
  it("holds where the core says the token comes from", async () => {
    vi.mocked(commands.tokenSource).mockResolvedValue("gh");
    const account = new AccountState();
    expect(account.source).toBeUndefined();
    await account.load();
    expect(account.source).toBe("gh");
    expect(account.error).toBeUndefined();
  });

  it("holds the error when the core can't be reached", async () => {
    vi.mocked(commands.tokenSource).mockRejectedValue(new Error("no IPC outside Tauri"));
    const account = new AccountState();
    await account.load();
    expect(account.source).toBeUndefined();
    expect(account.error).toBe("Error: no IPC outside Tauri");
  });
});
