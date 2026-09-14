import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { beforeEach, expect, it, vi } from "vitest";
import type { ShortcutAction, ShortcutBinding } from "./types";

const DEFAULTS: [ShortcutAction, string][] = [
  ["new-message", "Meta+N"],
  ["reply", "Meta+R"],
  ["reply-all", "Shift+Meta+R"],
  ["forward", "Shift+Meta+F"],
  ["archive", "Ctrl+Meta+A"],
  ["trash", "Backspace"],
  ["toggle-read", "Shift+Meta+U"],
  ["check-mail", "Shift+Meta+N"],
  ["toggle-sidebar", "Ctrl+Meta+S"],
  ["focus-search", "Meta+F"],
  ["send", "Meta+Enter"],
];

function defaults(): ShortcutBinding[] {
  return DEFAULTS.map(([action, combo]) => ({
    action,
    combo,
    defaultCombo: combo,
  }));
}

/** The defaults with some actions' combos replaced. */
function withCombos(
  changes: Partial<Record<ShortcutAction, string | null>>,
): ShortcutBinding[] {
  return defaults().map((binding) =>
    binding.action in changes
      ? { ...binding, combo: changes[binding.action] ?? null }
      : binding,
  );
}

let stored: ShortcutBinding[];

vi.mock("./api", () => ({
  getShortcuts: vi.fn(async () => stored),
  setShortcut: vi.fn(async () => stored),
  resetShortcuts: vi.fn(async () => defaults()),
}));

import * as api from "./api";
import ShortcutsPane from "./ShortcutsPane.svelte";

beforeEach(() => {
  vi.clearAllMocks();
  stored = defaults();
});

async function chipFor(label: string) {
  return await screen.findByRole("button", {
    name: `Change shortcut for ${label}`,
  });
}

it("lists every action with its shortcut in Mac glyphs", async () => {
  stored = withCombos({ trash: null });
  render(ShortcutsPane);

  expect(await chipFor("New Message")).toHaveTextContent("⌘N");
  expect(await chipFor("Archive")).toHaveTextContent("⌃⌘A");
  expect(await chipFor("Send (compose window)")).toHaveTextContent("⌘↩");
  expect(await chipFor("Move to Trash")).toHaveTextContent("None");
});

it("records the next key combination pressed", async () => {
  render(ShortcutsPane);
  const chip = await chipFor("Reply");
  vi.mocked(api.setShortcut).mockResolvedValueOnce(
    withCombos({ reply: "Alt+Meta+R" }),
  );

  await fireEvent.click(chip);
  expect(chip).toHaveTextContent("Press keys…");
  // A lone modifier is not a shortcut yet — keep listening.
  await fireEvent.keyDown(document.body, { key: "Meta", code: "MetaLeft", metaKey: true });
  expect(api.setShortcut).not.toHaveBeenCalled();
  await fireEvent.keyDown(document.body, {
    key: "®",
    code: "KeyR",
    altKey: true,
    metaKey: true,
  });

  expect(api.setShortcut).toHaveBeenCalledWith("reply", "Alt+Meta+R");
  await waitFor(() => expect(chip).toHaveTextContent("⌥⌘R"));
});

it("says which action lost the combination to the new one", async () => {
  render(ShortcutsPane);
  vi.mocked(api.setShortcut).mockResolvedValueOnce(
    withCombos({ reply: "Meta+N", "new-message": null }),
  );

  await fireEvent.click(await chipFor("Reply"));
  await fireEvent.keyDown(document.body, { key: "n", metaKey: true });

  expect(await screen.findByText("⌘N removed from New Message")).toBeInTheDocument();
  expect(await chipFor("New Message")).toHaveTextContent("None");
});

it("cancels recording on Escape without closing the window", async () => {
  const windowKeys = vi.fn();
  window.addEventListener("keydown", windowKeys);
  render(ShortcutsPane);
  const chip = await chipFor("Reply");

  await fireEvent.click(chip);
  await fireEvent.keyDown(document.body, { key: "Escape", code: "Escape" });

  expect(chip).toHaveTextContent("⌘R");
  expect(api.setShortcut).not.toHaveBeenCalled();
  // The settings window closes on Escape — recording must swallow it.
  expect(windowKeys).not.toHaveBeenCalled();
  window.removeEventListener("keydown", windowKeys);
});

it("shows a rejected combination inline", async () => {
  render(ShortcutsPane);
  vi.mocked(api.setShortcut).mockRejectedValueOnce(
    "macOS already uses this shortcut.",
  );

  await fireEvent.click(await chipFor("Reply"));
  await fireEvent.keyDown(document.body, { key: "q", metaKey: true });

  expect(await screen.findByRole("alert")).toHaveTextContent(
    "⌘Q: macOS already uses this shortcut.",
  );
  expect(await chipFor("Reply")).toHaveTextContent("⌘R");
});

it("unbinds with the clear button", async () => {
  render(ShortcutsPane);
  vi.mocked(api.setShortcut).mockResolvedValueOnce(withCombos({ forward: null }));

  await fireEvent.click(
    await screen.findByRole("button", { name: "Clear shortcut for Forward" }),
  );

  expect(api.setShortcut).toHaveBeenCalledWith("forward", null);
  await waitFor(async () =>
    expect(await chipFor("Forward")).toHaveTextContent("None"),
  );
});

it("offers to restore a single default only once it differs", async () => {
  stored = withCombos({ reply: "Alt+Meta+R" });
  render(ShortcutsPane);
  await chipFor("Reply");

  expect(
    screen.queryByRole("button", { name: "Restore default for Forward" }),
  ).not.toBeInTheDocument();
  vi.mocked(api.setShortcut).mockResolvedValueOnce(defaults());
  await fireEvent.click(
    screen.getByRole("button", { name: "Restore default for Reply" }),
  );

  expect(api.setShortcut).toHaveBeenCalledWith("reply", "Meta+R");
  await waitFor(() =>
    expect(
      screen.queryByRole("button", { name: "Restore default for Reply" }),
    ).not.toBeInTheDocument(),
  );
});

it("restores every default at once", async () => {
  stored = withCombos({ reply: "Alt+Meta+R", trash: null });
  render(ShortcutsPane);
  expect(await chipFor("Move to Trash")).toHaveTextContent("None");

  await fireEvent.click(screen.getByRole("button", { name: "Restore Defaults" }));

  expect(api.resetShortcuts).toHaveBeenCalled();
  await waitFor(async () =>
    expect(await chipFor("Move to Trash")).toHaveTextContent("⌫"),
  );
});

it("lists the fixed shortcuts that cannot be changed", async () => {
  render(ShortcutsPane);

  expect(await screen.findByText("Fixed")).toBeInTheDocument();
  expect(screen.getByText("⌘,")).toBeInTheDocument();
});
