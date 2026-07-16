import { describe, expect, it } from "vitest";
import {
  accumulateOffset,
  actionFor,
  isHorizontal,
  SWIPE_MAX,
  SWIPE_TRIGGER,
} from "./swipe";

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
  it("fires archive after a full swipe left", () => {
    expect(actionFor(-SWIPE_TRIGGER)).toBe("archive");
    expect(actionFor(-SWIPE_MAX)).toBe("archive");
  });

  it("fires the read toggle after a full swipe right", () => {
    expect(actionFor(SWIPE_TRIGGER)).toBe("toggleRead");
  });

  it("fires nothing when released short of the trigger line", () => {
    expect(actionFor(0)).toBeNull();
    expect(actionFor(-SWIPE_TRIGGER + 1)).toBeNull();
    expect(actionFor(SWIPE_TRIGGER - 1)).toBeNull();
  });
});
