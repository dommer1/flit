// Pure accounting for the pull-to-refresh gesture at the top of the message
// list, kept apart from the component so the thresholds are unit-testable
// (same split as swipe.ts for the row swipes).

/** Pull depth (px) a gesture must reach for its release to start a refresh. */
export const PULL_TRIGGER = 60;

/** Rubber-band limit (px) — the indicator never opens further than this. */
export const PULL_MAX = 90;

/** Fraction of each wheel pulse that feeds the pull — the drag "resists",
 * so the indicator feels elastic instead of tracking the fingers 1:1. */
export const PULL_RESISTANCE = 0.5;

/**
 * Fold one wheel event into the pull depth. Fingers moving up (scrolling
 * past the top) arrive as negative deltaY and deepen the pull; turning back
 * down releases it. Clamped to [0, PULL_MAX].
 */
export function accumulatePull(offset: number, deltaY: number): number {
  return Math.max(0, Math.min(PULL_MAX, offset - deltaY * PULL_RESISTANCE));
}

/** Whether a gesture released at `offset` starts a refresh. */
export function shouldRefresh(offset: number): boolean {
  return offset >= PULL_TRIGGER;
}
