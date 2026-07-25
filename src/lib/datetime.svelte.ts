// The app-wide date/time display preference, kept in one shared rune
// instead of threaded through props: it is read deep in the tree (list rows,
// conversation cards) and by plain modules (draft.ts's reply attribution),
// where a prop cannot reach.
//
// why no invoke() here: the module stays dependency-free so format.ts — and
// its tests — can import it without dragging in the Tauri IPC layer. The
// window that owns the fetch calls applyDateTimeFormat with what it read.

import { SYSTEM_DATE_TIME_FORMAT, type DateTimeFormat } from "./types";

/** The format in force. Reading it inside a component or a $derived
 * subscribes that render to later changes. */
export const dateTimeFormat: DateTimeFormat = $state({
  ...SYSTEM_DATE_TIME_FORMAT,
});

/** Adopt the stored preference. Mutates in place — reassigning the export
 * would leave every holder of the old object stuck on the old value. */
export function applyDateTimeFormat(next: DateTimeFormat): void {
  dateTimeFormat.date = next.date;
  dateTimeFormat.time = next.time;
}
