import { expect, it } from "vitest";
import { prerender, ssr } from "./+layout";

it("renders in the webview, with no server", () => {
  expect(ssr).toBe(false);
  expect(prerender).toBe(false);
});
