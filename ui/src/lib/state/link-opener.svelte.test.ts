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

it("drops an answer that a newer page, or a forget, came after", async () => {
  const links = new LinkOpener();
  let failLate: (problem: string) => void = () => {};
  const late = new Promise<string>((resolve) => (failLate = resolve));
  vi.mocked(openLink).mockReturnValueOnce(late).mockResolvedValueOnce(undefined);

  const first = links.openLink("https://example.com/a");
  await links.openLink("https://example.com/b");
  failLate("Error: no browser");
  await first;
  const afterNewer = links.problem;
  vi.mocked(openOnGitHub).mockReturnValueOnce(late);
  const forgotten = links.openOnGitHub("python/cpython", { kind: "issue", number: 7 });
  links.forget();
  await forgotten;

  expect([afterNewer, links.problem]).toEqual([undefined, undefined]);
});
