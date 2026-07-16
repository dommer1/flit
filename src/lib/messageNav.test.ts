import { describe, expect, it } from "vitest";
import { neighborId, nextMessageId } from "./messageNav";

describe("nextMessageId", () => {
  const ids = [10, 20, 30];

  it("returns null when there are no messages", () => {
    expect(nextMessageId([], null, 1)).toBeNull();
    expect(nextMessageId([], 10, -1)).toBeNull();
  });

  it("selects the first message on down / last on up when nothing is selected", () => {
    expect(nextMessageId(ids, null, 1)).toBe(10);
    expect(nextMessageId(ids, null, -1)).toBe(30);
  });

  it("moves to the neighbour in the given direction", () => {
    expect(nextMessageId(ids, 10, 1)).toBe(20);
    expect(nextMessageId(ids, 30, -1)).toBe(20);
  });

  it("stays put at the boundaries instead of wrapping", () => {
    expect(nextMessageId(ids, 30, 1)).toBe(30);
    expect(nextMessageId(ids, 10, -1)).toBe(10);
  });

  it("falls back to an edge when the current id is gone from the list", () => {
    expect(nextMessageId(ids, 999, 1)).toBe(10);
    expect(nextMessageId(ids, 999, -1)).toBe(30);
  });
});

describe("neighborId", () => {
  const ids = [10, 20, 30];

  it("picks the message below the removed one", () => {
    expect(neighborId(ids, 20)).toBe(30);
  });

  it("picks the one above when the removed one was last", () => {
    expect(neighborId(ids, 30)).toBe(20);
  });

  it("returns null when the list would empty", () => {
    expect(neighborId([10], 10)).toBeNull();
  });

  it("returns null when the id isn't in the list", () => {
    expect(neighborId(ids, 999)).toBeNull();
  });
});
