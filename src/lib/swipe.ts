// Pure accounting for the two-finger (trackpad) swipe on message-list rows,
// kept apart from the component so the thresholds are unit-testable.

/** Offset (px) a gesture must reach for its action to fire on release. */
export const SWIPE_TRIGGER = 72;

/** Rubber-band limit (px) — the row never travels further than this. */
export const SWIPE_MAX = 96;

export type SwipeAction = "archive" | "toggleRead";

/**
 * Fold one wheel event into a row offset, clamped to the rubber band.
 * Positive offset = row pushed right. With natural scrolling, fingers moving
 * left arrive as positive deltaX, so the offset runs against the delta.
 */
export function accumulateOffset(offset: number, deltaX: number): number {
  return Math.max(-SWIPE_MAX, Math.min(SWIPE_MAX, offset - deltaX));
}

/** Whether a horizontal gesture owns this wheel event (vs vertical scroll). */
export function isHorizontal(deltaX: number, deltaY: number): boolean {
  return Math.abs(deltaX) > Math.abs(deltaY);
}

/** The action a gesture released at `offset` fires; null short of the line. */
export function actionFor(offset: number): SwipeAction | null {
  if (offset <= -SWIPE_TRIGGER) return "archive";
  if (offset >= SWIPE_TRIGGER) return "toggleRead";
  return null;
}
