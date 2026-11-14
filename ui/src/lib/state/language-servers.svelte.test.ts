import { expect, it, vi } from "vitest";
import type { ServerRow } from "$lib/data/bindings";
import { LanguageServerRows } from "./language-servers.svelte";

const core = vi.hoisted(() => ({
  languageServers: vi.fn(),
  heard: (payload: unknown) => {
    void payload;
  },
}));

vi.mock("$lib/data/bindings", () => ({
  commands: { languageServers: core.languageServers },
  events: {
    languageServersChanged: {
      listen: async (listener: (event: { payload: unknown }) => void) => {
        core.heard = (payload) => {
          listener({ payload });
        };
        return () => {};
      },
    },
  },
}));

const row = (state: "ready" | "running"): ServerRow => ({
  server: "ty",
  status: { state },
  version: null,
  tested: ["0.0.103"],
});

it("keeps a row the core sent while it was asked, over the older answer", async () => {
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
