import { expect, it } from "vitest";
import {
  accumulatePull,
  PULL_MAX,
  PULL_RESISTANCE,
  PULL_TRIGGER,
  shouldRefresh,
} from "./pullRefresh";

it("grows the pull against upward wheel deltas, scaled by resistance", () => {
  expect(accumulatePull(0, -40)).toBe(40 * PULL_RESISTANCE);
});

it("shrinks the pull when the wheel turns back down", () => {
  expect(accumulatePull(30, 20)).toBe(30 - 20 * PULL_RESISTANCE);
});

it("never pulls below zero", () => {
  expect(accumulatePull(10, 1000)).toBe(0);
});

it("rubber-bands at the maximum", () => {
  expect(accumulatePull(PULL_MAX, -1000)).toBe(PULL_MAX);
});

it("keeps the trigger reachable inside the rubber band", () => {
  expect(PULL_TRIGGER).toBeLessThanOrEqual(PULL_MAX);
});

it("refreshes only at or past the trigger line", () => {
  expect(shouldRefresh(PULL_TRIGGER - 1)).toBe(false);
  expect(shouldRefresh(PULL_TRIGGER)).toBe(true);
});
