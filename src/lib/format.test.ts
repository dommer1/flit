import { describe, expect, it } from "vitest";
import {
  formatFileSize,
  formatFullDate,
  formatListDate,
  sectionFor,
  senderInitials,
  senderName,
} from "./format";

// Local-time ISO strings (no Z) keep the tests timezone-independent.
const now = new Date(2026, 6, 9, 15, 0); // Thursday 9 July 2026, 15:00

describe("senderName", () => {
  it("extracts the display name from name-addr form", () => {
    expect(senderName("Alice <alice@example.com>")).toBe("Alice");
  });

  it("strips surrounding quotes", () => {
    expect(senderName('"Mery, Dominik" <domco@example.com>')).toBe(
      "Mery, Dominik",
    );
  });

  it("falls back to the raw address when there is no display name", () => {
    expect(senderName("alice@example.com")).toBe("alice@example.com");
  });
});

describe("senderInitials", () => {
  it("takes the first letter of the first two name words", () => {
    expect(senderInitials("Alžbeta Ostrihoňová <a@example.com>")).toBe("AO");
  });

  it("uses a single initial for one-word names", () => {
    expect(senderInitials("Alice <alice@example.com>")).toBe("A");
  });

  it("uses the first letter of a bare address", () => {
    expect(senderInitials("alice@example.com")).toBe("A");
  });

  it("returns a placeholder when nothing usable remains", () => {
    expect(senderInitials("")).toBe("?");
  });
});

describe("formatListDate", () => {
  it("shows the time for messages from today", () => {
    const expected = new Intl.DateTimeFormat(undefined, {
      timeStyle: "short",
    }).format(new Date(2026, 6, 9, 10, 6));
    expect(formatListDate("2026-07-09T10:06:00", now)).toBe(expected);
  });

  it("shows Yesterday for messages from the previous day", () => {
    expect(formatListDate("2026-07-08T23:59:00", now)).toBe("Yesterday");
  });

  it("shows the weekday within the last week", () => {
    const expected = new Intl.DateTimeFormat(undefined, {
      weekday: "long",
    }).format(new Date(2026, 6, 6));
    expect(formatListDate("2026-07-06T08:00:00", now)).toBe(expected);
  });

  it("shows a numeric date for older messages", () => {
    const expected = new Intl.DateTimeFormat(undefined, {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
    }).format(new Date(2026, 4, 8));
    expect(formatListDate("2026-05-08T19:51:00", now)).toBe(expected);
  });

  it("returns the raw string for unparseable dates", () => {
    expect(formatListDate("not a date", now)).toBe("not a date");
  });
});

describe("sectionFor", () => {
  // now is Thursday 9 July 2026 — the current (Monday-start) week began
  // Mon 6 July, the previous week ran Mon 29 June – Sun 5 July.
  it("groups today's messages under Today", () => {
    expect(sectionFor("2026-07-09T00:01:00", now)).toBe("Today");
  });

  it("puts future-dated messages (clock skew) under Today too", () => {
    expect(sectionFor("2026-07-10T09:00:00", now)).toBe("Today");
  });

  it("groups the previous day under Yesterday", () => {
    expect(sectionFor("2026-07-08T23:59:00", now)).toBe("Yesterday");
  });

  it("groups earlier days of the current week under This Week", () => {
    expect(sectionFor("2026-07-06T00:00:00", now)).toBe("This Week");
    expect(sectionFor("2026-07-07T12:00:00", now)).toBe("This Week");
  });

  it("groups the previous calendar week under Last Week", () => {
    expect(sectionFor("2026-07-05T23:59:00", now)).toBe("Last Week");
    expect(sectionFor("2026-06-29T00:00:00", now)).toBe("Last Week");
  });

  it("keeps Last Week days that spill into the previous month", () => {
    expect(sectionFor("2026-06-30T10:00:00", now)).toBe("Last Week");
  });

  it("groups current-month days older than last week under This Month", () => {
    // Tuesday 28 July 2026: last week ran 20–26 July, so 10 July is left
    // for the month bucket.
    const lateJuly = new Date(2026, 6, 28, 15, 0);
    expect(sectionFor("2026-07-10T09:00:00", lateJuly)).toBe("This Month");
  });

  it("labels older months of the current year with month and year", () => {
    const expected = new Intl.DateTimeFormat(undefined, {
      month: "long",
      year: "numeric",
    }).format(new Date(2026, 4, 8));
    expect(sectionFor("2026-05-08T19:51:00", now)).toBe(expected);
  });

  it("labels previous years with the year alone", () => {
    expect(sectionFor("2025-12-31T23:59:00", now)).toBe("2025");
    expect(sectionFor("2024-01-05T08:00:00", now)).toBe("2024");
  });

  it("falls back to Unknown for unparseable dates", () => {
    expect(sectionFor("not a date", now)).toBe("Unknown");
  });
});

describe("formatFullDate", () => {
  it("shows a long date with time", () => {
    const expected = new Intl.DateTimeFormat(undefined, {
      dateStyle: "long",
      timeStyle: "short",
    }).format(new Date(2026, 4, 8, 19, 51));
    expect(formatFullDate("2026-05-08T19:51:00")).toBe(expected);
  });

  it("returns the raw string for unparseable dates", () => {
    expect(formatFullDate("not a date")).toBe("not a date");
  });
});

describe("formatFileSize", () => {
  it("keeps byte counts plain below 1 kB", () => {
    expect(formatFileSize(0)).toBe("0 B");
    expect(formatFileSize(999)).toBe("999 B");
  });

  it("uses one decimal under 10 units and none above", () => {
    expect(formatFileSize(1200)).toBe("1.2 kB");
    expect(formatFileSize(45_600)).toBe("46 kB");
    expect(formatFileSize(2_400_000)).toBe("2.4 MB");
    expect(formatFileSize(123_000_000)).toBe("123 MB");
    expect(formatFileSize(1_100_000_000)).toBe("1.1 GB");
  });
});
