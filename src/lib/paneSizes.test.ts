import { describe, expect, it } from "vitest";
import {
  clampPaneWidth,
  DEFAULT_PANE_WIDTHS,
  loadPaneWidths,
  PANE_WIDTHS_KEY,
  savePaneWidths,
} from "./paneSizes";

function fakeStorage(initial: Record<string, string> = {}) {
  const data = new Map(Object.entries(initial));
  return {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => void data.set(key, value),
    data,
  };
}

describe("clampPaneWidth", () => {
  it("keeps widths inside the pane's limits", () => {
    expect(clampPaneWidth("sidebar", 50)).toBe(140);
    expect(clampPaneWidth("sidebar", 9999)).toBe(400);
    expect(clampPaneWidth("list", 100)).toBe(240);
    expect(clampPaneWidth("list", 9999)).toBe(640);
  });

  it("passes through widths already in range", () => {
    expect(clampPaneWidth("sidebar", 250)).toBe(250);
    expect(clampPaneWidth("list", 400)).toBe(400);
  });
});

describe("loadPaneWidths", () => {
  it("returns defaults when nothing is stored", () => {
    expect(loadPaneWidths(fakeStorage())).toEqual(DEFAULT_PANE_WIDTHS);
  });

  it("returns stored widths", () => {
    const storage = fakeStorage({
      [PANE_WIDTHS_KEY]: JSON.stringify({ sidebar: 250, list: 400 }),
    });
    expect(loadPaneWidths(storage)).toEqual({ sidebar: 250, list: 400 });
  });

  it("clamps stored widths that are out of range", () => {
    const storage = fakeStorage({
      [PANE_WIDTHS_KEY]: JSON.stringify({ sidebar: 5, list: 9999 }),
    });
    expect(loadPaneWidths(storage)).toEqual({ sidebar: 140, list: 640 });
  });

  it("falls back to defaults on corrupt or partial data", () => {
    expect(
      loadPaneWidths(fakeStorage({ [PANE_WIDTHS_KEY]: "not json" })),
    ).toEqual(DEFAULT_PANE_WIDTHS);
    expect(
      loadPaneWidths(
        fakeStorage({ [PANE_WIDTHS_KEY]: JSON.stringify({ sidebar: "x" }) }),
      ),
    ).toEqual(DEFAULT_PANE_WIDTHS);
  });
});

describe("savePaneWidths", () => {
  it("round-trips through loadPaneWidths", () => {
    const storage = fakeStorage();
    savePaneWidths(storage, { sidebar: 300, list: 500 });
    expect(loadPaneWidths(storage)).toEqual({ sidebar: 300, list: 500 });
  });
});
