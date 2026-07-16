// Display formatting for message headers, mirroring Apple Mail conventions:
// today → time, yesterday → "Yesterday", this week → weekday, older → date.

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

export function formatListDate(iso: string, now: Date = new Date()): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  const startOfDay = (d: Date) =>
    new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const dayDiff = Math.round(
    (startOfDay(now) - startOfDay(date)) / 86_400_000,
  );
  if (dayDiff <= 0) {
    return new Intl.DateTimeFormat(undefined, { timeStyle: "short" }).format(
      date,
    );
  }
  if (dayDiff === 1) return "Yesterday";
  if (dayDiff < 7) {
    return new Intl.DateTimeFormat(undefined, { weekday: "long" }).format(
      date,
    );
  }
  return new Intl.DateTimeFormat(undefined, {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).format(date);
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

export function formatFullDate(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "long",
    timeStyle: "short",
  }).format(date);
}
