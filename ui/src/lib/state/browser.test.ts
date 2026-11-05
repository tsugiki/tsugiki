import { beforeEach, expect, it, vi } from "vitest";
import { commands } from "$lib/data/bindings";
import { openLink, openOnGitHub } from "./browser";

vi.mock("$lib/data/bindings", () => ({
  commands: { openOnGithub: vi.fn(), openLink: vi.fn() },
}));

beforeEach(() => {
  vi.resetAllMocks();
});

it("asks the core to open an item on GitHub, and has nothing to say when it does", async () => {
  vi.mocked(commands.openOnGithub).mockResolvedValue({ status: "ok", data: null });

  const problem = await openOnGitHub("python/cpython", { kind: "issue", number: 7 });

  expect(commands.openOnGithub).toHaveBeenCalledWith("python/cpython", { kind: "issue", number: 7 });
  expect(problem).toBeUndefined();
});

it("says why the core couldn't open a link", async () => {
  vi.mocked(commands.openLink).mockResolvedValue({
    status: "error",
    error: { kind: "internal", message: "Tsugiki opens only web links in the browser." },
  });

  expect(await openLink("file:///etc/passwd")).toBe("Tsugiki opens only web links in the browser.");
  expect(commands.openLink).toHaveBeenCalledWith("file:///etc/passwd");
});

it("says so when the core doesn't answer at all", async () => {
  vi.mocked(commands.openLink).mockRejectedValue(new Error("no core"));

  expect(await openLink("https://github.com/")).toBe("Error: no core");
});
