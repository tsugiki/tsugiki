import { expect, it } from "vitest";
import { Visits } from "./visits.svelte";

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
  expect([visits.behind(12), visits.ahead(12)]).toEqual([[], []]);
});

it("lists up to a limit of places each way, the nearest first", () => {
  const visits = visited("#1", "#2", "#3", "#4", "#5");
  visits.back();
  visits.back();

  expect(visits.behind(12)).toEqual(["#2", "#1"]);
  expect(visits.behind(1)).toEqual(["#2"]);
  expect(visits.ahead(12)).toEqual(["#4", "#5"]);
  expect(visits.ahead(1)).toEqual(["#4"]);
});

it("jumps several places at once, and nowhere past either end", () => {
  const visits = visited("#1", "#2", "#3", "#4");

  expect(visits.go(-3)).toBe("#1");
  expect(visits.go(-1)).toBeUndefined();
  expect(visits.go(2)).toBe("#3");
  expect(visits.go(5)).toBeUndefined();
  expect(visits.behind(12)).toEqual(["#2", "#1"]);
});
