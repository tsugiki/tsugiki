import { expect, test, vi } from "vitest";
import { AboutState } from "./about.svelte";

vi.mock("$lib/data/bindings", () => ({
  commands: { about: async () => ({ name: "Tsugiki", version: "1.2.3" }) },
}));

test("AboutState", async ({ bench }) => {
  // Reading the import inside the loop would add the test runner's module lookup to what is measured.
  const State = AboutState;

  // load() keeps a failure to itself, so a broken mock would otherwise measure only the failure path.
  const state = new State();
  await state.load();
  expect(state.about).toBeDefined();

  await bench("load", async () => {
    await new State().load();
  }).run();
});
