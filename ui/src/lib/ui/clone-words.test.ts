import { describe, expect, it } from "vitest";
import { pickedWords, refusalWords } from "./clone-words";

const folder = String.raw`D:\cpython`;

describe("refusalWords", () => {
  it.each([
    [{ kind: "not_a_folder" }, String.raw`D:\cpython isn't a folder.`],
    [
      { kind: "not_a_clone", git_says: "not a git repository" },
      String.raw`D:\cpython isn't a git clone: not a git repository`,
    ],
    [{ kind: "no_remote" }, String.raw`D:\cpython has no remote for python/cpython.`],
    [{ kind: "shallow" }, String.raw`D:\cpython is a shallow clone, which git can't borrow from.`],
  ] as const)("words %o", (refusal, words) => {
    expect(refusalWords(folder, "python/cpython", refusal)).toBe(words);
  });
});

describe("pickedWords", () => {
  it("names the clone that lends, the reason one can't, or what failed", () => {
    expect(pickedWords({ kind: "lends", folder }, "python/cpython")).toBe(
      String.raw`Borrows from D:\cpython`,
    );
    const refused = { kind: "refused", folder, refusal: { kind: "no_remote" } } as const;
    expect(pickedWords(refused, "python/cpython")).toBe(
      String.raw`D:\cpython has no remote for python/cpython.`,
    );
    expect(pickedWords({ kind: "failed", problem: "no picker" }, "python/cpython")).toBe("no picker");
  });

  it("says nothing when the user cancelled", () => {
    expect(pickedWords({ kind: "cancelled" }, "python/cpython")).toBeNull();
  });
});
