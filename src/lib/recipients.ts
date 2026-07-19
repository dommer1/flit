// Recipient-list plumbing for the compose envelope: splitting a
// comma-separated header list into individual addresses and joining the
// chip row back into that wire format. Pure functions — the chip field
// component stays thin.

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

/** The comma-separated string a chip row stands for: the committed chips
 * plus whatever address is still being typed after them. */
export function joinRecipients(chips: string[], text: string): string {
  if (chips.length === 0) return text;
  return text === "" ? chips.join(", ") : `${chips.join(", ")}, ${text}`;
}
