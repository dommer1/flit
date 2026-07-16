// Date math for the "Send Later" popover. Pure functions over local time —
// the backend stores unix seconds (UTC), the conversion happens here.

export interface SendLaterPreset {
  label: string;
  date: Date;
}

/** Quick picks for the popover, relative to `now`; already-passed moments
 * drop out, so the list never offers a time in the past. */
export function presets(now: Date): SendLaterPreset[] {
  const evening = new Date(now);
  evening.setHours(18, 0, 0, 0);
  const tomorrow = new Date(now);
  tomorrow.setDate(tomorrow.getDate() + 1);
  tomorrow.setHours(8, 0, 0, 0);

  const picks = [
    { label: "This evening 18:00", date: evening },
    { label: "Tomorrow 8:00", date: tomorrow },
  ];
  return picks.filter((p) => p.date > now);
}

/** Parse a datetime-local input value ("2026-07-15T18:00", local time) into
 * unix seconds. */
export function toEpochSeconds(value: string): number {
  // why: without a timezone suffix Date parses this format as local time,
  // which is exactly what a datetime-local input means.
  return Math.floor(new Date(value).getTime() / 1000);
}

/** Format a date as a datetime-local input value (local time, minutes). */
export function toDatetimeLocal(date: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return (
    `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}` +
    `T${pad(date.getHours())}:${pad(date.getMinutes())}`
  );
}
