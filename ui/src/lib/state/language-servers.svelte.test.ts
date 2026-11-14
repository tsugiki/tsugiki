import { expect, it, vi } from "vitest";
import type { ServerRow } from "$lib/data/bindings";
import { LanguageServerRows, typedPath } from "./language-servers.svelte";

type Listener = (event: { payload: unknown }) => void;

const core = vi.hoisted(() => ({
  languageServers: vi.fn(),
  heard: (payload: unknown) => {
    void payload;
  },
  listen: vi.fn(),
}));

/** Listens at once, as Tauri does once its listener is in place. */
function listening(listener: Listener): Promise<() => void> {
  core.heard = (payload) => {
    listener({ payload });
  };
  return Promise.resolve(() => {});
}

vi.mock("$lib/data/bindings", () => ({
  commands: { languageServers: core.languageServers },
  events: { languageServersChanged: { listen: core.listen } },
}));

const row = (state: "ready" | "running"): ServerRow => ({
  server: "ty",
  status: { state },
  version: null,
  tested: ["0.0.103"],
});

it("keeps a row the core sent while it was asked, over the older answer", async () => {
  core.listen.mockImplementation(listening);
  const answer = Promise.withResolvers<ServerRow[]>();
  core.languageServers.mockReturnValue(answer.promise);
  const rows = new LanguageServerRows();
  const stop = rows.watch();
  await vi.waitFor(() => {
    expect(core.languageServers).toHaveBeenCalledOnce();
  });

  core.heard([row("running")]);
  answer.resolve([row("ready")]);
  await answer.promise;
  await Promise.resolve();
  stop();

  expect(rows.row("ty")?.status).toEqual({ state: "running" });
  expect(rows.row("clangd")).toBeUndefined();
});

it("asks for the rows only once it listens, so a change the core sends in between isn't lost", async () => {
  core.languageServers.mockReset();
  const inPlace = Promise.withResolvers<() => void>();
  core.listen.mockImplementation((listener: Listener) => {
    void listening(listener);
    return inPlace.promise;
  });
  core.languageServers.mockResolvedValue([row("ready")]);
  const rows = new LanguageServerRows();

  const stop = rows.watch();
  await Promise.resolve();
  const askedEarly = core.languageServers.mock.calls.length;
  inPlace.resolve(() => {});
  await vi.waitFor(() => {
    expect(rows.row("ty")?.status).toEqual({ state: "ready" });
  });
  stop();

  expect(askedEarly).toBe(0);
});

it("says why it can't hear the core's changes, and still asks for the rows", async () => {
  core.listen.mockRejectedValue(new Error("no event system"));
  core.languageServers.mockResolvedValue([row("ready")]);
  const rows = new LanguageServerRows();

  const stop = rows.watch();
  await vi.waitFor(() => {
    expect(rows.row("ty")?.status).toEqual({ state: "ready" });
  });
  stop();

  expect(rows.problem).toBe("Tsugiki can't hear how the language servers change: Error: no event system");
});

it("takes a path without the spaces or the one pair of quotes around it, and a blank one as none", () => {
  expect(typedPath(" C:/llvm/bin/clangd.exe ")).toBe("C:/llvm/bin/clangd.exe");
  expect(typedPath('"C:/Program Files/LLVM/bin/clangd.exe"')).toBe("C:/Program Files/LLVM/bin/clangd.exe");
  expect(typedPath('"C:/llvm/bin/clangd.exe')).toBe('"C:/llvm/bin/clangd.exe');
  expect(typedPath('""')).toBeNull();
  expect(typedPath("   ")).toBeNull();
});
