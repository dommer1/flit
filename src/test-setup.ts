import "@testing-library/jest-dom/vitest";

// jsdom doesn't implement scrollIntoView; stub it so components that keep the
// selected row visible don't crash under test.
Element.prototype.scrollIntoView = () => {};
