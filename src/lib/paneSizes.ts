/**
 * Widths (px) of the two fixed columns in the three-pane layout; the third
 * column takes the remaining space. Persisted in localStorage — layout
 * preferences are per-device UI state, not account data, so they don't
 * belong in SQLite.
 */
export interface PaneWidths {
  sidebar: number;
  list: number;
}

export const PANE_WIDTHS_KEY = "flit.paneWidths";

export const DEFAULT_PANE_WIDTHS: PaneWidths = { sidebar: 208, list: 352 };

export const PANE_LIMITS: Record<keyof PaneWidths, { min: number; max: number }> =
  {
    sidebar: { min: 140, max: 400 },
    list: { min: 240, max: 640 },
  };

export function clampPaneWidth(pane: keyof PaneWidths, width: number): number {
  const { min, max } = PANE_LIMITS[pane];
  return Math.min(max, Math.max(min, width));
}

type StorageLike = Pick<Storage, "getItem" | "setItem">;

export function loadPaneWidths(storage: StorageLike): PaneWidths {
  try {
    const raw = storage.getItem(PANE_WIDTHS_KEY);
    if (raw === null) return { ...DEFAULT_PANE_WIDTHS };
    const parsed: unknown = JSON.parse(raw);
    const { sidebar, list } = parsed as Partial<Record<string, unknown>>;
    if (typeof sidebar !== "number" || typeof list !== "number") {
      return { ...DEFAULT_PANE_WIDTHS };
    }
    return {
      sidebar: clampPaneWidth("sidebar", sidebar),
      list: clampPaneWidth("list", list),
    };
  } catch {
    return { ...DEFAULT_PANE_WIDTHS };
  }
}

export function savePaneWidths(storage: StorageLike, widths: PaneWidths): void {
  storage.setItem(PANE_WIDTHS_KEY, JSON.stringify(widths));
}
