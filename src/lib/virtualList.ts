// Which slice of a long list is worth putting in the DOM.
//
// why this exists: a big folder hands the list its full page of 500 rows, and
// rendering all of them measured 85-878 ms — in one folder more than the
// database query that produced them. Only about twenty are ever on screen.
//
// why arithmetic rather than measuring each row: every row is the same two
// lines since the body preview came out of the list, so where a row sits is a
// running sum, not something the browser has to be asked. Measuring 500 rows
// to decide which 20 to draw would cost what it saves.

/** Running top offset of every item, plus the total height as a final entry.
 * `offsets[i]` is where item `i` starts; `offsets[heights.length]` is the
 * whole list's height. */
export function offsetsFor(heights: number[]): number[] {
  const offsets = new Array<number>(heights.length + 1);
  offsets[0] = 0;
  for (let i = 0; i < heights.length; i += 1) {
    offsets[i + 1] = offsets[i] + heights[i];
  }
  return offsets;
}

/** The slice to render, and the empty space standing in for the rest. */
export interface ListWindow {
  /** First item to render. */
  start: number;
  /** One past the last item to render. */
  end: number;
  /** Pixels standing in for everything before `start`. */
  padTop: number;
  /** Pixels standing in for everything after `end`. */
  padBottom: number;
}

/** Height assumed while the viewport has not been measured yet — enough to
 * cover a tall window, so the first paint is never short of rows. */
const UNMEASURED_VIEWPORT = 1200;

/** Index of the last item whose top satisfies `keep`, by binary search over
 * the running offsets. `keep` is monotonic: true for a prefix, then false. */
function lastWhere(offsets: number[], keep: (top: number) => boolean): number {
  let low = 0;
  let high = offsets.length - 2; // last real item
  while (low < high) {
    const mid = (low + high + 1) >> 1;
    if (keep(offsets[mid])) low = mid;
    else high = mid - 1;
  }
  return low;
}

/**
 * Which items to draw for a viewport `height` tall scrolled to `scrollTop`,
 * plus `overscan` items of margin on each side so a fast scroll does not
 * expose blank space before the next frame.
 */
export function windowFor(
  offsets: number[],
  scrollTop: number,
  height: number,
  overscan: number,
): ListWindow {
  const count = offsets.length - 1;
  if (count <= 0) return { start: 0, end: 0, padTop: 0, padBottom: 0 };

  // why not zero: before the element is measured its clientHeight reads 0,
  // and taking that at face value would render nothing at all on first paint.
  const viewport = height > 0 ? height : UNMEASURED_VIEWPORT;
  const top = Math.max(0, Math.min(scrollTop, offsets[count]));

  // The item the viewport's top edge falls inside — it is partly on screen.
  const first = lastWhere(offsets, (t) => t <= top);
  // why strictly before the bottom edge: an item starting exactly at it has
  // no pixel on screen, and drawing it would make every window one row wider
  // than it needs to be.
  const bottom = top + viewport;
  const last = lastWhere(offsets, (t) => t < bottom);

  const start = Math.max(0, first - overscan);
  const end = Math.min(count, last + 1 + overscan);
  return {
    start,
    end,
    padTop: offsets[start],
    padBottom: offsets[count] - offsets[end],
  };
}
