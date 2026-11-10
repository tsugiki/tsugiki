import { beforeEach, expect, it, vi } from "vitest";
import { pickClone } from "./mirror-source";
import { MirrorChoosing } from "./mirror-choice.svelte";

vi.mock("./mirror-source", () => ({ pickClone: vi.fn() }));

const lender = String.raw`D:\gcmon`;
const refused = { kind: "refused", folder: lender, refusal: { kind: "no_remote" } } as const;

beforeEach(() => {
  vi.resetAllMocks();
});

it("downloads the mirror unless the user picks a clone that lends", async () => {
  const choosing = new MirrorChoosing();
  expect(choosing.source).toEqual({ kind: "download" });

  vi.mocked(pickClone).mockResolvedValue({ kind: "lends", folder: lender });
  await choosing.chooseClone("Some-Maintainer/gcmon");
  expect(choosing.source).toEqual({ kind: "borrow", folder: lender });
  expect(pickClone).toHaveBeenCalledWith("Some-Maintainer/gcmon");

  choosing.chooseDownload();
  expect(choosing.source).toEqual({ kind: "download" });
});

it("has no source while the picker is open or the clone can't lend", async () => {
  const choosing = new MirrorChoosing();
  vi.mocked(pickClone).mockResolvedValue(refused);

  const picking = choosing.chooseClone("Some-Maintainer/gcmon");
  expect(choosing.choice).toEqual({ kind: "borrow", picked: undefined });
  expect(choosing.source).toBeUndefined();
  await picking;

  expect(choosing.choice).toEqual({ kind: "borrow", picked: refused });
  expect(choosing.source).toBeUndefined();
});

it("keeps what was chosen when the user cancels the picker", async () => {
  const choosing = new MirrorChoosing();
  vi.mocked(pickClone).mockResolvedValue({ kind: "lends", folder: lender });
  await choosing.chooseClone("Some-Maintainer/gcmon");
  vi.mocked(pickClone).mockResolvedValue({ kind: "cancelled" });
  await choosing.chooseClone("Some-Maintainer/gcmon");
  expect(choosing.source).toEqual({ kind: "borrow", folder: lender });
});
