// Pure accounting for the two-finger (trackpad) swipe on message-list rows,
// kept apart from the component so the thresholds are unit-testable.

import type { SwipeAction, SwipeActions } from "./types";

/** Offset (px) a gesture must reach for its action to fire on release. */
export const SWIPE_TRIGGER = 72;

/** Rubber-band limit (px) — the row never travels further than this. */
export const SWIPE_MAX = 96;

/** The gesture as originally shipped: left archives, right toggles read. */
export const DEFAULT_SWIPE_ACTIONS: SwipeActions = {
  left: "archive",
  right: "toggleRead",
};

/**
 * Fold one wheel event into a row offset, clamped to the rubber band.
 * Positive offset = row pushed right. With natural scrolling, fingers moving
 * left arrive as positive deltaX, so the offset runs against the delta.
 * A side configured to "none" is pinned at 0 — the row doesn't budge.
 */
export function accumulateOffset(
  offset: number,
  deltaX: number,
  actions: SwipeActions = DEFAULT_SWIPE_ACTIONS,
): number {
  const min = actions.left === "none" ? 0 : -SWIPE_MAX;
  const max = actions.right === "none" ? 0 : SWIPE_MAX;
  return Math.max(min, Math.min(max, offset - deltaX));
}

/** Whether a horizontal gesture owns this wheel event (vs vertical scroll). */
export function isHorizontal(deltaX: number, deltaY: number): boolean {
  return Math.abs(deltaX) > Math.abs(deltaY);
}

/** The configured action a gesture released at `offset` fires;
 * null short of the trigger line or on a side set to "none". */
export function actionFor(
  offset: number,
  actions: SwipeActions = DEFAULT_SWIPE_ACTIONS,
): SwipeAction | null {
  const action =
    offset <= -SWIPE_TRIGGER
      ? actions.left
      : offset >= SWIPE_TRIGGER
        ? actions.right
        : null;
  return action === "none" ? null : action;
}
