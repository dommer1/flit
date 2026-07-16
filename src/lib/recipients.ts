// Token accounting for comma-separated recipient fields: which fragment the
// user is typing (feeds the contact lookup) and how a picked suggestion
// replaces it. Pure functions — the dropdown component stays thin.

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
