// Opt-in timing for the performance work, mirroring the backend's
// FLIT_TIMING switch (src-tauri/src/timing.rs).
//
// Switched on from devtools with
//   localStorage.setItem("flit:timing", "1")
// and off again with "0" or by removing the key. The flag is read per call
// rather than cached, so it can be flipped mid-session without a reload —
// a localStorage read is nothing next to the IPC round trips being measured.

/** Is timing output switched on right now? */
export function timingEnabled(): boolean {
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
    console.info(formatLine(label, performance.now() - start));
  });
}

/** Report how long the browser takes to reach its next frame. Called right
 * after a state assignment, it measures what that assignment cost to
 * render — the number FLI-23 (list virtualization) has to move. */
export function markPaint(label: string): void {
  if (!timingEnabled()) return;
  const start = performance.now();
  requestAnimationFrame(() => {
    console.info(formatLine(label, performance.now() - start));
  });
}
