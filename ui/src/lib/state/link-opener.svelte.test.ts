import { beforeEach, expect, it, vi } from "vitest";
import { openLink, openOnGitHub } from "./browser";
import { LinkOpener } from "./link-opener.svelte";

vi.mock("./browser", () => ({ openLink: vi.fn(), openOnGitHub: vi.fn() }));

beforeEach(() => {
  vi.resetAllMocks();
});

it("keeps what kept the browser from the last page, until the next opens or it forgets", async () => {
  const links = new LinkOpener();
  vi.mocked(openOnGitHub).mockResolvedValue("Error: no browser");

  await links.openOnGitHub("python/cpython", { kind: "issue", number: 7 });
  const failed = links.problem;
  vi.mocked(openLink).mockResolvedValue(undefined);
  await links.openLink("https://github.com/");
  const opened = links.problem;
  vi.mocked(openLink).mockResolvedValue("Error: no browser");
  await links.openLink("https://github.com/");
  links.forget();

  expect([failed, opened, links.problem]).toEqual(["Error: no browser", undefined, undefined]);
  expect(openOnGitHub).toHaveBeenCalledWith("python/cpython", { kind: "issue", number: 7 });
});
