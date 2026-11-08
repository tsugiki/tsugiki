import { expect, it } from "vitest";
import { Visits } from "./visits";

const visited = (...places: string[]) => {
  const visits = new Visits<string>((one, other) => one === other);
  for (const place of places) visits.visit(place);
  return visits;
};

it("goes back and forward through the places visited, and no further than either end", () => {
  const visits = visited("#1", "#2", "#3");

  expect([visits.back(), visits.back(), visits.back()]).toEqual(["#2", "#1", undefined]);
  expect([visits.forward(), visits.forward(), visits.forward()]).toEqual(["#2", "#3", undefined]);
});

it("drops the places ahead once the user visits a new one from an earlier place", () => {
  const visits = visited("#1", "#2", "#3");
  visits.back();
  visits.back();

  visits.visit("#4");

  expect(visits.forward()).toBeUndefined();
  expect([visits.back(), visits.back()]).toEqual(["#1", undefined]);
});

it("keeps one place for a place visited again at once", () => {
  const visits = visited("#1", "#1", "#2", "#2");

  expect([visits.back(), visits.back()]).toEqual(["#1", undefined]);
});

it("has nowhere to go before the first visit", () => {
  const visits = visited();

  expect([visits.back(), visits.forward()]).toEqual([undefined, undefined]);
});
