import { describe, expect, it } from "vitest";
import {
  accumulateOffset,
  actionFor,
  DEFAULT_SWIPE_ACTIONS,
  isHorizontal,
  SWIPE_MAX,
  SWIPE_TRIGGER,
} from "./swipe";
import type { SwipeActions } from "./types";

describe("accumulateOffset", () => {
  it("moves the row against the wheel delta (natural scrolling)", () => {
    // Fingers moving left produce positive deltaX — the row follows them left.
    expect(accumulateOffset(0, 30)).toBe(-30);
    expect(accumulateOffset(-30, 30)).toBe(-60);
    // Fingers moving right push the row right.
    expect(accumulateOffset(0, -30)).toBe(30);
  });

  it("clamps the offset to the rubber-band maximum on both sides", () => {
    expect(accumulateOffset(-SWIPE_MAX, 50)).toBe(-SWIPE_MAX);
    expect(accumulateOffset(SWIPE_MAX, -50)).toBe(SWIPE_MAX);
  });

  it("lets a gesture reverse back through zero", () => {
    expect(accumulateOffset(-40, -60)).toBe(20);
  });

  it("pins the row on a side whose action is none", () => {
    const leftOff: SwipeActions = { left: "none", right: "toggleRead" };
    expect(accumulateOffset(0, 30, leftOff)).toBe(0);
    expect(accumulateOffset(0, -30, leftOff)).toBe(30);

    const rightOff: SwipeActions = { left: "archive", right: "none" };
    expect(accumulateOffset(0, -30, rightOff)).toBe(0);
    expect(accumulateOffset(0, 30, rightOff)).toBe(-30);
  });
});

describe("isHorizontal", () => {
  it("claims the event only when deltaX dominates", () => {
    expect(isHorizontal(12, 3)).toBe(true);
    expect(isHorizontal(-12, 3)).toBe(true);
    expect(isHorizontal(3, 12)).toBe(false);
    // A perfect diagonal stays with vertical scrolling.
    expect(isHorizontal(5, 5)).toBe(false);
  });
});

describe("actionFor", () => {
  it("defaults to the shipped mapping: left archives, right toggles read", () => {
    expect(actionFor(-SWIPE_TRIGGER)).toBe("archive");
    expect(actionFor(-SWIPE_MAX)).toBe("archive");
    expect(actionFor(SWIPE_TRIGGER)).toBe("toggleRead");
    expect(DEFAULT_SWIPE_ACTIONS).toEqual({
      left: "archive",
      right: "toggleRead",
    });
  });

  it("fires whatever action each direction is configured with", () => {
    const actions: SwipeActions = { left: "trash", right: "reply" };
    expect(actionFor(-SWIPE_TRIGGER, actions)).toBe("trash");
    expect(actionFor(SWIPE_TRIGGER, actions)).toBe("reply");
  });

  it("fires nothing on a side configured to none", () => {
    const actions: SwipeActions = { left: "none", right: "none" };
    expect(actionFor(-SWIPE_MAX, actions)).toBeNull();
    expect(actionFor(SWIPE_MAX, actions)).toBeNull();
  });

  it("fires nothing when released short of the trigger line", () => {
    expect(actionFor(0)).toBeNull();
    expect(actionFor(-SWIPE_TRIGGER + 1)).toBeNull();
    expect(actionFor(SWIPE_TRIGGER - 1)).toBeNull();
  });
});
