import { expect, it } from "vitest";
import { offsetsFor, windowFor } from "./virtualList";

/** Ten 100px rows — round numbers keep the expectations readable. */
const TEN = offsetsFor(new Array(10).fill(100));

it("turns heights into running offsets and a total", () => {
  expect(offsetsFor([10, 20, 30])).toEqual([0, 10, 30, 60]);
});

it("handles an empty list", () => {
  expect(windowFor(offsetsFor([]), 0, 500, 2)).toEqual({
    start: 0,
    end: 0,
    padTop: 0,
    padBottom: 0,
  });
});

it("draws the visible rows and pads out the rest", () => {
  // Viewport 250px at the top: rows 0,1,2 are on screen, no overscan.
  expect(windowFor(TEN, 0, 250, 0)).toEqual({
    start: 0,
    end: 3,
    padTop: 0,
    padBottom: 700,
  });
});

it("counts a row the viewport only partly covers", () => {
  // 50..300 shows the bottom half of row 0, so row 0 is drawn. Row 3 starts
  // at exactly 300 and has no pixel on screen, so it is not.
  expect(windowFor(TEN, 50, 250, 0)).toMatchObject({ start: 0, end: 3 });
});

it("keeps overscan rows on each side", () => {
  const w = windowFor(TEN, 400, 200, 2);

  expect(w).toMatchObject({ start: 2, end: 8 });
  expect(w.padTop).toBe(200);
  expect(w.padBottom).toBe(200);
});

it("does not overscan past either end", () => {
  expect(windowFor(TEN, 0, 200, 5)).toMatchObject({ start: 0, padTop: 0 });
  expect(windowFor(TEN, 800, 200, 5)).toMatchObject({
    end: 10,
    padBottom: 0,
  });
});

it("draws rows even before the viewport has been measured", () => {
  // why: a bound element reads clientHeight 0 until it lays out. Believing
  // that would render an empty list on first paint.
  const w = windowFor(TEN, 0, 0, 0);

  expect(w.start).toBe(0);
  expect(w.end).toBeGreaterThan(1);
});

it("survives a scroll position past the end", () => {
  // Shrinking the list under a scrolled viewport (an eviction, a folder
  // switch) briefly leaves scrollTop beyond the new total.
  const w = windowFor(TEN, 99999, 200, 1);

  expect(w.end).toBe(10);
  expect(w.padBottom).toBe(0);
});

it("copes with rows of different heights", () => {
  // Date headers are shorter than rows, so offsets are not a fixed stride.
  const offsets = offsetsFor([30, 100, 100, 30, 100]);

  expect(windowFor(offsets, 0, 130, 0)).toMatchObject({ start: 0, end: 2 });
  expect(windowFor(offsets, 230, 100, 0)).toMatchObject({ start: 3, end: 5 });
});
