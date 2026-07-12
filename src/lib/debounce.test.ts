import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { debounce } from "./debounce";

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

it("collapses rapid calls into one trailing call with the latest arguments", () => {
  const fn = vi.fn();
  const run = debounce(fn, 200);

  run("a");
  run("b");
  run("c");

  vi.advanceTimersByTime(199);
  expect(fn).not.toHaveBeenCalled();
  vi.advanceTimersByTime(1);
  expect(fn).toHaveBeenCalledTimes(1);
  expect(fn).toHaveBeenCalledWith("c");
});

it("fires again for calls after the quiet period", () => {
  const fn = vi.fn();
  const run = debounce(fn, 200);

  run("a");
  vi.advanceTimersByTime(200);
  run("b");
  vi.advanceTimersByTime(200);

  expect(fn).toHaveBeenCalledTimes(2);
  expect(fn).toHaveBeenLastCalledWith("b");
});
