import { logTiming } from "./api";

// Opt-in timing for the performance work, mirroring the backend's
// FLIT_TIMING switch (src-tauri/src/timing.rs).
//
// Switched on from devtools with
//   localStorage.setItem("flit:timing", "1")
// and off again with "0" or by removing the key. The flag is read per call
// rather than cached, so it can be flipped mid-session without a reload —
// a localStorage read is nothing next to the IPC round trips being measured.

// Whether the backend process runs with FLIT_TIMING set. Seeded once at
// startup (see main.ts) so a single env var covers both halves of a click:
// a release build has no devtools in which to set the local flag, and a
// release build is where the numbers have to come from.
let backendTiming = false;

/** Adopt the backend's switch. */
export function adoptBackendTiming(on: boolean): void {
  backendTiming = on;
}

/** Is timing output switched on right now? */
export function timingEnabled(): boolean {
  if (backendTiming) return true;
  try {
    const flag = localStorage.getItem("flit:timing");
    return flag !== null && flag !== "" && flag !== "0";
  } catch {
    // why swallow: a webview with storage blocked must not break the app
    // over a debug switch.
    return false;
  }
}

/** One line of output — same shape as the backend's, so both halves of a
 * click can be read together in one place. */
export function formatLine(label: string, ms: number): string {
  return `[timing] ${label} ${ms.toFixed(1)} ms`;
}

/** Emit one line.
 *
 * why it goes to the backend as well as the console: a webview's console
 * goes to the webview, and a release build — the only build worth measuring
 * — has no devtools to open it with. Handing the line back puts it on the
 * process's stderr, in the one stream `npm run timing` captures.
 *
 * why fire-and-forget: the line already carries its own measurement, so when
 * it lands does not matter; waiting for it would add IPC latency to the very
 * path being measured. */
function report(line: string): void {
  console.info(line);
  if (backendTiming) void logTiming(line).catch(() => {});
}

/** Run `work`, reporting how long it took. Returns whatever it returns, and
 * still reports when it throws — a slow failure is a measurement too.
 *
 * why not declared `async`: an async wrapper would hand back a fresh promise
 * and insert extra microtask ticks into every wrapped call even with timing
 * switched off. Changing when things settle is exactly what an instrument
 * must never do — switched off, this returns the caller's own promise
 * untouched. */
export function timed<T>(label: string, work: () => Promise<T>): Promise<T> {
  if (!timingEnabled()) return work();
  const start = performance.now();
  return work().finally(() => {
    report(formatLine(label, performance.now() - start));
  });
}

/** Past this, the frame did not arrive because the window stopped
 * rendering — backgrounded, occluded, or napped by the OS — not because the
 * render was slow. No list render takes a second. */
const PAINT_STALL_MS = 1000;

/** Report how long the browser takes to reach its next frame. Called right
 * after a state assignment, it measures what that assignment cost to
 * render — the number FLI-23 (list virtualization) has to move.
 *
 * why the stall guard: macOS throttles requestAnimationFrame to nothing for
 * a window that is not on screen, so a backgrounded app produced "paint"
 * readings of 38 and 171 SECONDS. Reported plainly those numbers invite
 * exactly the wrong conclusion, so a frame that never came is labelled as
 * what it is rather than passed off as render cost. */
export function markPaint(label: string): void {
  if (!timingEnabled()) return;
  const start = performance.now();
  requestAnimationFrame(() => {
    const ms = performance.now() - start;
    if (ms > PAINT_STALL_MS) {
      report(`[timing] ${label} stalled ${ms.toFixed(0)} ms (window not rendering)`);
      return;
    }
    report(formatLine(label, ms));
  });
}
