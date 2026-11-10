import { beforeEach, expect, it, vi } from "vitest";
import { commands } from "$lib/data/bindings";
import { chooseMirrorSource, mirrorCost, pickClone } from "./mirror-source";

vi.mock("$lib/data/bindings", () => ({
  commands: { pickFolder: vi.fn(), checkClone: vi.fn(), chooseMirrorSource: vi.fn(), mirrorCost: vi.fn() },
}));

it("gives what a mirror takes, and asks GitHub again only once it answers", async () => {
  const cost = { mirror_mb: 891, download_seconds: 209, borrow_seconds: 36 };
  const offline = { kind: "offline", message: "Tsugiki can't reach GitHub." } as const;
  vi.mocked(commands.mirrorCost).mockResolvedValueOnce({ status: "error", error: offline });
  expect(await mirrorCost("python/peps")).toBeNull();

  vi.mocked(commands.mirrorCost).mockResolvedValueOnce({ status: "ok", data: cost });
  expect(await mirrorCost("python/peps")).toEqual(cost);
  expect(await mirrorCost("python/peps")).toEqual(cost);

  expect(commands.mirrorCost).toHaveBeenCalledTimes(2);
  expect(commands.mirrorCost).toHaveBeenCalledWith("python/peps");
});

const folder = String.raw`D:\cpython`;

beforeEach(() => {
  vi.resetAllMocks();
});

it("checks the folder the user picks as a clone of the Repository", async () => {
  vi.mocked(commands.pickFolder).mockResolvedValue({ status: "ok", data: folder });
  vi.mocked(commands.checkClone).mockResolvedValue({ status: "ok", data: null });

  expect(await pickClone("python/cpython")).toEqual({ kind: "lends", folder });
  expect(commands.checkClone).toHaveBeenCalledWith("python/cpython", folder);
});

it("gives the reason a folder can't lend", async () => {
  vi.mocked(commands.pickFolder).mockResolvedValue({ status: "ok", data: folder });
  vi.mocked(commands.checkClone).mockResolvedValue({ status: "ok", data: { kind: "no_remote" } });

  expect(await pickClone("python/cpython")).toEqual({
    kind: "refused",
    folder,
    refusal: { kind: "no_remote" },
  });
});

it("checks nothing when the user cancels", async () => {
  vi.mocked(commands.pickFolder).mockResolvedValue({ status: "ok", data: null });

  expect(await pickClone("python/cpython")).toEqual({ kind: "cancelled" });
  expect(commands.checkClone).not.toHaveBeenCalled();
});

it("says why the picker or the check failed", async () => {
  const refused = { status: "error", error: { kind: "internal", message: "no picker" } } as const;
  vi.mocked(commands.pickFolder).mockResolvedValueOnce(refused);
  const noPicker = await pickClone("python/cpython");
  vi.mocked(commands.pickFolder).mockResolvedValue({ status: "ok", data: folder });
  vi.mocked(commands.checkClone).mockRejectedValue(new Error("no core"));
  const noCheck = await pickClone("python/cpython");

  expect(noPicker).toEqual({ kind: "failed", problem: "no picker" });
  expect(noCheck).toEqual({ kind: "failed", problem: "Error: no core" });
});

it("saves the answer, and says why it couldn't", async () => {
  vi.mocked(commands.chooseMirrorSource).mockResolvedValueOnce({ status: "ok", data: null });
  const saved = await chooseMirrorSource("python/cpython", { kind: "download" });
  vi.mocked(commands.chooseMirrorSource).mockRejectedValue(new Error("no core"));
  const failed = await chooseMirrorSource("python/cpython", { kind: "download" });

  expect(saved).toBeUndefined();
  expect(commands.chooseMirrorSource).toHaveBeenCalledWith("python/cpython", { kind: "download" });
  expect(failed).toBe("Error: no core");
});
