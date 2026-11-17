import { expect, it } from "vitest";
import { counted } from "./counted";

it("says one with the singular, and any other count with the plural in thousands with commas", () => {
  expect([counted(1, "file", "files"), counted(0, "file", "files"), counted(1204, "file", "files")]).toEqual([
    "1 file",
    "0 files",
    "1,204 files",
  ]);
});
