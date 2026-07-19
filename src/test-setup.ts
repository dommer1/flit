import "@testing-library/jest-dom/vitest";

// jsdom doesn't implement scrollIntoView; stub it so components that keep the
// selected row visible don't crash under test.
Element.prototype.scrollIntoView = () => {};

// jsdom doesn't implement ResizeObserver either; a no-op keeps the
// auto-sizing message cards from crashing (and failing the run) under test.
class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}
globalThis.ResizeObserver = ResizeObserverStub as unknown as typeof ResizeObserver;
