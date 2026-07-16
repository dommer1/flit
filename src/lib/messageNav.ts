// Keyboard navigation for the message list: pick the id to select when the
// user presses Arrow Up / Arrow Down. Kept pure and separate from App.svelte
// so the boundary/edge behaviour is unit-tested without a DOM.

/** `+1` = down (next), `-1` = up (previous). */
export type NavDelta = 1 | -1;

/**
 * The id to select when moving `delta` from `currentId` within `ids`:
 * - empty list → null (nothing to select);
 * - nothing selected → first on down, last on up;
 * - current id missing (e.g. just deleted) → the same edge fallback;
 * - otherwise the neighbour, clamped at the ends (no wrap-around).
 */
export function nextMessageId(
  ids: readonly number[],
  currentId: number | null,
  delta: NavDelta,
): number | null {
  if (ids.length === 0) return null;
  const index = currentId === null ? -1 : ids.indexOf(currentId);
  if (index === -1) return delta > 0 ? ids[0] : ids[ids.length - 1];
  const next = Math.min(Math.max(index + delta, 0), ids.length - 1);
  return ids[next];
}

/**
 * Which message to select after `removedId` leaves the list (deleted /
 * moved to trash): the one below it, or the one above if it was last, or
 * null when the list empties or the id wasn't present.
 */
export function neighborId(
  ids: readonly number[],
  removedId: number,
): number | null {
  const index = ids.indexOf(removedId);
  if (index === -1) return null;
  return ids[index + 1] ?? ids[index - 1] ?? null;
}
