import { expect, it } from "vitest";
import { presets, toDatetimeLocal, toEpochSeconds } from "./sendLater";

// Fixed "now" values — a mid-morning and a late-evening moment.
const morning = new Date(2026, 6, 15, 10, 30);
const evening = new Date(2026, 6, 15, 20, 0);

it("offers this evening and tomorrow morning during the day", () => {
  const picks = presets(morning);

  expect(picks.map((p) => p.label)).toEqual([
    "This evening 18:00",
    "Tomorrow 8:00",
  ]);
  expect(picks[0].date).toEqual(new Date(2026, 6, 15, 18, 0, 0, 0));
  expect(picks[1].date).toEqual(new Date(2026, 6, 16, 8, 0, 0, 0));
});

it("drops this evening once 18:00 has passed", () => {
  const picks = presets(evening);

  expect(picks.map((p) => p.label)).toEqual(["Tomorrow 8:00"]);
  expect(picks[0].date).toEqual(new Date(2026, 6, 16, 8, 0, 0, 0));
});

it("converts a datetime-local string to local-time epoch seconds", () => {
  const chosen = new Date(2026, 6, 15, 18, 0);

  expect(toEpochSeconds("2026-07-15T18:00")).toBe(
    Math.floor(chosen.getTime() / 1000),
  );
});

it("formats a date as a datetime-local value, zero-padded", () => {
  expect(toDatetimeLocal(new Date(2026, 0, 5, 8, 5))).toBe("2026-01-05T08:05");
});

it("round-trips datetime-local values", () => {
  const value = "2026-07-15T18:00";

  expect(toDatetimeLocal(new Date(toEpochSeconds(value) * 1000))).toBe(value);
});
