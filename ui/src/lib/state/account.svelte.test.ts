import { describe, expect, it, vi } from "vitest";
import { commands } from "$lib/data/bindings";
import { AccountState, signedIn } from "./account.svelte";

vi.mock("$lib/data/bindings", () => ({ commands: { tokenSource: vi.fn() } }));

describe("signedIn", () => {
  it("names the login GitHub confirmed", () => {
    expect(signedIn({ state: "online", login: "octocat" })).toEqual({ login: "octocat" });
  });

  it("names last time's login, and says so, while GitHub hasn't answered or can't be reached", () => {
    expect(signedIn({ state: "connecting_as", last_login: "octocat" })).toEqual({
      login: "octocat",
      note: "Signed in last time. GitHub hasn't answered yet.",
    });
    expect(signedIn({ state: "offline_as", last_login: "octocat" })).toEqual({
      login: "octocat",
      note: "Signed in last time. GitHub can't be reached now.",
    });
  });

  it("says why it names nobody", () => {
    expect(signedIn(undefined)).toEqual({ note: "Not known yet: GitHub hasn't answered." });
    expect(signedIn({ state: "connecting" })).toEqual({ note: "Not known yet: GitHub hasn't answered." });
    expect(signedIn({ state: "offline" })).toEqual({ note: "Not known: GitHub can't be reached." });
    expect(
      signedIn({ state: "problem", problem: { kind: "unauthorized", message: "Run gh auth login." } }),
    ).toEqual({ note: "Run gh auth login." });
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
