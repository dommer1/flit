import { fireEvent, render, screen } from "@testing-library/svelte";
import { beforeEach, expect, it, vi } from "vitest";
import type { McpSettings, McpStatus } from "./types";

let stored: McpSettings;
let status: McpStatus;

vi.mock("./api", () => ({
  getMcpSettings: vi.fn(async () => stored),
  setMcpSettings: vi.fn(async (next: McpSettings) => {
    stored = next;
    return next.enabled
      ? { running: true, port: next.port, error: null }
      : { running: false, port: null, error: null };
  }),
  mcpStatus: vi.fn(async () => status),
  copyMcpSetup: vi.fn(async () => undefined),
  regenerateMcpToken: vi.fn(async () => status),
}));

import * as api from "./api";
import McpSettingsPane from "./McpSettings.svelte";

beforeEach(() => {
  vi.clearAllMocks();
  stored = { enabled: false, access: "read", port: 4729 };
  status = { running: false, port: null, error: null };
});

const ON: McpSettings = { enabled: true, access: "read", port: 4729 };

it("is switched off by default and shows nothing else", async () => {
  render(McpSettingsPane);

  expect(await screen.findByLabelText(/Let AI apps use Flit/)).not.toBeChecked();
  expect(screen.queryByLabelText("Access")).not.toBeInTheDocument();
});

it("says plainly that what a connected app reads leaves Flit", async () => {
  render(McpSettingsPane);

  expect(
    await screen.findByText(/usually to its maker's cloud/),
  ).toBeInTheDocument();
});

it("saves switching it on and shows where it listens", async () => {
  render(McpSettingsPane);

  await fireEvent.click(await screen.findByLabelText(/Let AI apps use Flit/));

  expect(api.setMcpSettings).toHaveBeenCalledWith(ON);
  expect(
    await screen.findByText("http://127.0.0.1:4729/mcp"),
  ).toBeInTheDocument();
});

it("rolls the switch back and shows why when saving fails", async () => {
  vi.mocked(api.setMcpSettings).mockRejectedValueOnce("keychain says no");
  render(McpSettingsPane);
  const toggle = await screen.findByLabelText(/Let AI apps use Flit/);

  await fireEvent.click(toggle);

  expect(await screen.findByRole("alert")).toHaveTextContent("keychain says no");
  expect(toggle).not.toBeChecked();
});

it("stores the access level a client gets", async () => {
  stored = { ...ON };
  status = { running: true, port: 4729, error: null };
  render(McpSettingsPane);

  await fireEvent.change(await screen.findByLabelText("Access"), {
    target: { value: "full" },
  });

  expect(api.setMcpSettings).toHaveBeenCalledWith({ ...ON, access: "full" });
  expect(await screen.findByText(/without asking/)).toBeInTheDocument();
});

it("stores a new port", async () => {
  stored = { ...ON };
  render(McpSettingsPane);

  await fireEvent.change(await screen.findByLabelText("Port"), {
    target: { value: "5100" },
  });

  expect(api.setMcpSettings).toHaveBeenCalledWith({ ...ON, port: 5100 });
});

it("shows why the server is not running", async () => {
  stored = { ...ON };
  status = { running: false, port: null, error: "Port 4729 is not available" };
  render(McpSettingsPane);

  expect(
    await screen.findByText("Port 4729 is not available"),
  ).toBeInTheDocument();
});

it("copies the connection details through the backend", async () => {
  stored = { ...ON };
  status = { running: true, port: 4729, error: null };
  render(McpSettingsPane);

  await fireEvent.click(
    await screen.findByRole("button", { name: "Copy Claude Code command" }),
  );
  await fireEvent.click(screen.getByRole("button", { name: "Copy JSON config" }));
  await fireEvent.click(screen.getByRole("button", { name: "Copy token" }));

  expect(vi.mocked(api.copyMcpSetup).mock.calls).toEqual([
    ["claude-code"],
    ["json"],
    ["token"],
  ]);
  expect(await screen.findByText("Copied")).toBeInTheDocument();
});

it("replaces the token on request", async () => {
  stored = { ...ON };
  status = { running: true, port: 4729, error: null };
  render(McpSettingsPane);

  await fireEvent.click(await screen.findByRole("button", { name: "New token" }));

  expect(api.regenerateMcpToken).toHaveBeenCalled();
});
