// Token accounting for comma-separated recipient fields: which fragment the
// user is typing (feeds the contact lookup) and how a picked suggestion
// replaces it. Pure functions — the dropdown component stays thin.

/** Split a display list on commas, except commas inside a display name —
 * a piece without an @ belongs to the name before the address
 * (`"Novák, Ján" <jan@x>`), so it is glued back onto the previous piece. */
export function splitRecipients(list: string): string[] {
  const entries: string[] = [];
  for (const piece of list.split(",")) {
    const trimmed = piece.trim();
    if (trimmed === "") continue;
    const last = entries.length - 1;
    if (last >= 0 && !entries[last].includes("@")) {
      entries[last] += `, ${trimmed}`;
    } else {
      entries.push(trimmed);
    }
  }
  return entries;
}

/** The recipient fragment being typed at `caret`: everything between the
 * last comma before the caret and the caret. */
export function activeTerm(value: string, caret: number): string {
  const before = value.slice(0, caret);
  return before.slice(before.lastIndexOf(",") + 1).trim();
}

/** Replace the fragment around `caret` with `email`, preserving the other
 * recipients on both sides. */
export function applySuggestion(
  value: string,
  caret: number,
  email: string,
): string {
  const start = value.slice(0, caret).lastIndexOf(",") + 1;
  const nextComma = value.indexOf(",", caret);
  const end = nextComma === -1 ? value.length : nextComma;
  const prefix = value.slice(0, start);
  const suffix = value.slice(end);
  return `${prefix}${prefix === "" ? "" : " "}${email}${suffix}`;
}
