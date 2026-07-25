// Display formatting for message headers, mirroring Apple Mail conventions:
// today → time, yesterday → "Yesterday", this week → weekday, older → date.
//
// Numeric dates and clock times follow the user's preference (Settings →
// General); "system" means the OS locale decides, which is the default and
// what every one of these functions falls back to.

import { dateTimeFormat } from "./datetime.svelte";
import { type DateFormat, type DateTimeFormat, type TimeFormat } from "./types";

export function senderName(from: string): string {
  const match = from.match(/^\s*"?(.*?)"?\s*<[^<>]*>\s*$/);
  const name = match?.[1]?.trim();
  return name || from.trim();
}

export function senderInitials(from: string): string {
  const words = senderName(from)
    .split(/\s+/)
    .map((word) => [...word].find((ch) => /\p{L}|\p{N}/u.test(ch)))
    .filter((ch): ch is string => ch !== undefined);
  return words.slice(0, 2).join("").toUpperCase() || "?";
}

/** The clock alone, on the preferred cycle. */
function clock(date: Date, time: TimeFormat): string {
  // why hourCycle instead of hour12: false — under hour12: false some
  // locales write midnight as 24:15; h23 pins it to 00:15 everywhere.
  const options: Intl.DateTimeFormatOptions =
    time === "24h"
      ? { hour: "2-digit", minute: "2-digit", hourCycle: "h23" }
      : time === "12h"
        ? { hour: "numeric", minute: "2-digit", hour12: true }
        : { timeStyle: "short" };
  return new Intl.DateTimeFormat(undefined, options).format(date);
}

/** The date as a bare numeric pattern, or null when the locale decides.
 * Built from the local parts by hand: the pattern names its own output, so
 * a locale must not be able to reorder or re-punctuate it. */
function numericDate(date: Date, format: DateFormat): string | null {
  const dd = String(date.getDate()).padStart(2, "0");
  const mm = String(date.getMonth() + 1).padStart(2, "0");
  const yyyy = String(date.getFullYear()).padStart(4, "0");
  switch (format) {
    case "dd.mm.yyyy":
      return `${dd}.${mm}.${yyyy}`;
    case "dd.mm.yy":
      return `${dd}.${mm}.${yyyy.slice(-2)}`;
    case "dd/mm/yyyy":
      return `${dd}/${mm}/${yyyy}`;
    case "mm/dd/yyyy":
      return `${mm}/${dd}/${yyyy}`;
    case "yyyy-mm-dd":
      return `${yyyy}-${mm}-${dd}`;
    case "yyyy/mm/dd":
      return `${yyyy}/${mm}/${dd}`;
    case "system":
      return null;
  }
}

export function formatListDate(
  iso: string,
  now: Date = new Date(),
  format: DateTimeFormat = dateTimeFormat,
): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  const startOfDay = (d: Date) =>
    new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const dayDiff = Math.round(
    (startOfDay(now) - startOfDay(date)) / 86_400_000,
  );
  if (dayDiff <= 0) return clock(date, format.time);
  if (dayDiff === 1) return "Yesterday";
  if (dayDiff < 7) {
    return new Intl.DateTimeFormat(undefined, { weekday: "long" }).format(
      date,
    );
  }
  return (
    numericDate(date, format.date) ??
    new Intl.DateTimeFormat(undefined, {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
    }).format(date)
  );
}

// Date-section buckets for the message list, checked top-down: a message
// falls into the FIRST bucket that contains it. That resolves the overlap
// between relative and calendar buckets — "Last Week" keeps days that spill
// into the previous month, and "This Month" only holds what the day/week
// buckets left over.
export function sectionFor(iso: string, now: Date = new Date()): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "Unknown";
  const startOfDay = (d: Date) =>
    new Date(d.getFullYear(), d.getMonth(), d.getDate());
  const today = startOfDay(now);
  // Future dates (sender clock skew) count as today.
  if (date.getTime() >= today.getTime()) return "Today";
  const yesterday = new Date(today);
  yesterday.setDate(yesterday.getDate() - 1);
  if (date.getTime() >= yesterday.getTime()) return "Yesterday";
  // why Monday: ISO 8601 week start; Intl offers no portable week-start
  // lookup, and a fixed rule beats a locale-dependent section boundary.
  const thisWeek = new Date(today);
  thisWeek.setDate(thisWeek.getDate() - ((today.getDay() + 6) % 7));
  if (date.getTime() >= thisWeek.getTime()) return "This Week";
  const lastWeek = new Date(thisWeek);
  lastWeek.setDate(lastWeek.getDate() - 7);
  if (date.getTime() >= lastWeek.getTime()) return "Last Week";
  if (date.getFullYear() === now.getFullYear()) {
    if (date.getMonth() === now.getMonth()) return "This Month";
    return new Intl.DateTimeFormat(undefined, {
      month: "long",
      year: "numeric",
    }).format(date);
  }
  return String(date.getFullYear());
}

/** Human file size (decimal units, like Finder): 999 B, 1.2 kB, 46 kB, 2.4 MB. */
export function formatFileSize(bytes: number): string {
  if (bytes < 1000) return `${bytes} B`;
  let value = bytes;
  for (const unit of ["kB", "MB", "GB"]) {
    value /= 1000;
    if (value < 1000 || unit === "GB") {
      return `${value < 10 ? value.toFixed(1) : String(Math.round(value))} ${unit}`;
    }
  }
  return `${bytes} B`;
}

export function formatFullDate(
  iso: string,
  format: DateTimeFormat = dateTimeFormat,
): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  // why the untouched branch: with both halves on "system" the locale gets
  // to join the two itself ("May 8, 2026 at 7:51 PM"), and Intl rejects
  // dateStyle next to explicit hour options — so date and time can only be
  // combined by hand once either half is pinned.
  if (format.date === "system" && format.time === "system") {
    return new Intl.DateTimeFormat(undefined, {
      dateStyle: "long",
      timeStyle: "short",
    }).format(date);
  }
  const day =
    numericDate(date, format.date) ??
    new Intl.DateTimeFormat(undefined, { dateStyle: "long" }).format(date);
  return `${day} ${clock(date, format.time)}`;
}
