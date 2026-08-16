import { afterEach, expect, it, vi } from "vitest";
import { adoptBackendTiming, formatLine, timed, timingEnabled } from "./timing";

afterEach(() => {
  localStorage.removeItem("flit:timing");
  adoptBackendTiming(false);
  vi.restoreAllMocks();
});

it("follows the backend's FLIT_TIMING without any local flag", () => {
  // why: a release build ships without devtools, so there is no console in
  // which to set the localStorage flag — the env var has to reach the
  // webview by itself or the frontend half is unmeasurable where it counts.
  expect(timingEnabled()).toBe(false);

  adoptBackendTiming(true);
  expect(timingEnabled()).toBe(true);

  adoptBackendTiming(false);
  expect(timingEnabled()).toBe(false);
});

it("stays on when either side asks for it", () => {
  // The local flag remains a manual override for dev builds, where flipping
  // it in devtools beats restarting the app with a different environment.
  localStorage.setItem("flit:timing", "1");
  adoptBackendTiming(false);

  expect(timingEnabled()).toBe(true);
});

it("is off unless the flag says otherwise", () => {
  expect(timingEnabled()).toBe(false);

  localStorage.setItem("flit:timing", "1");
  expect(timingEnabled()).toBe(true);

  // "0" and "" read as off, so the flag can be parked without deleting it.
  localStorage.setItem("flit:timing", "0");
  expect(timingEnabled()).toBe(false);
  localStorage.setItem("flit:timing", "");
  expect(timingEnabled()).toBe(false);
});

it("formats a line the same way the backend does", () => {
  expect(formatLine("refreshMessages", 12.34)).toBe(
    "[timing] refreshMessages 12.3 ms",
  );
});

it("returns the work's value and stays quiet while switched off", async () => {
  const info = vi.spyOn(console, "info").mockImplementation(() => {});

  await expect(timed("quiet", async () => 42)).resolves.toBe(42);

  expect(info).not.toHaveBeenCalled();
});

it("hands back the caller's own promise while switched off", () => {
  // why identity, not just the value: wrapping in an async function would
  // return a fresh promise and push everything downstream a microtask later.
  // An instrument that shifts when things settle measures its own effect —
  // and it broke a MessageView test the first time round.
  const promise = Promise.resolve("untouched");

  expect(timed("passthrough", () => promise)).toBe(promise);
});

it("reports once switched on, and still reports on failure", async () => {
  localStorage.setItem("flit:timing", "1");
  const info = vi.spyOn(console, "info").mockImplementation(() => {});

  await expect(timed("ok", async () => "value")).resolves.toBe("value");
  // why also the failing path: a slow error is exactly the case we would
  // otherwise measure as "instant" and stop looking at.
  await expect(
    timed("boom", async () => {
      throw new Error("nope");
    }),
  ).rejects.toThrow("nope");

  expect(info).toHaveBeenCalledTimes(2);
  expect(info.mock.calls[0][0]).toMatch(/^\[timing\] ok /);
  expect(info.mock.calls[1][0]).toMatch(/^\[timing\] boom /);
});
