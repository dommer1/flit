import { render, screen, fireEvent, waitFor } from "@testing-library/svelte";
import { beforeEach, expect, it, vi } from "vitest";
import type { Account, NotificationSettings } from "./types";

function account(id: number, name: string, overrides: Partial<Account> = {}): Account {
  return {
    id,
    name,
    email: `${name.toLowerCase()}@example.com`,
    imapHost: "imap.example.com",
    imapPort: 993,
    smtpHost: "smtp.example.com",
    smtpPort: 587,
    username: `${name.toLowerCase()}@example.com`,
    lastError: null,
    checkedAt: null,
    color: null,
    signatureId: null,
    notifyEnabled: null,
    notifySound: null,
    defaultAliasId: null,
    ...overrides,
  };
}

let settings: NotificationSettings;

vi.mock("./api", () => ({
  getNotificationSettings: vi.fn(async () => settings),
  setNotificationSettings: vi.fn(async () => undefined),
  setAccountNotifications: vi.fn(async () => undefined),
  previewNotificationSound: vi.fn(async () => undefined),
}));

import * as api from "./api";
import NotificationsPane from "./NotificationsPane.svelte";

beforeEach(() => {
  vi.clearAllMocks();
  settings = {
    enabled: true,
    sound: "default",
    syncIntervalMinutes: 3,
    pushEnabled: false,
  };
});

it("shows the stored global settings", async () => {
  settings = {
    enabled: false,
    sound: "Ping",
    syncIntervalMinutes: 15,
    pushEnabled: false,
  };

  render(NotificationsPane, { props: { accounts: [] } });

  await waitFor(() =>
    expect(screen.getByLabelText("Show notifications")).not.toBeChecked(),
  );
  expect(screen.getByLabelText("Sound")).toHaveValue("Ping");
  expect(screen.getByLabelText("Check for new mail")).toHaveValue("15");
});

it("saves a global sound change", async () => {
  render(NotificationsPane, { props: { accounts: [] } });
  await waitFor(() =>
    expect(screen.getByLabelText("Show notifications")).toBeChecked(),
  );

  await fireEvent.change(screen.getByLabelText("Sound"), {
    target: { value: "Glass" },
  });

  await waitFor(() =>
    expect(api.setNotificationSettings).toHaveBeenCalledWith({
      enabled: true,
      sound: "Glass",
      syncIntervalMinutes: 3,
      pushEnabled: false,
    }),
  );
});

it("saves the check interval as a number", async () => {
  render(NotificationsPane, { props: { accounts: [] } });
  await waitFor(() =>
    expect(screen.getByLabelText("Check for new mail")).toHaveValue("3"),
  );

  await fireEvent.change(screen.getByLabelText("Check for new mail"), {
    target: { value: "0" },
  });

  await waitFor(() =>
    expect(api.setNotificationSettings).toHaveBeenCalledWith({
      enabled: true,
      sound: "default",
      syncIntervalMinutes: 0,
      pushEnabled: false,
    }),
  );
});

it("switching to automatic turns on push without losing the interval", async () => {
  render(NotificationsPane, { props: { accounts: [] } });
  await waitFor(() =>
    expect(screen.getByLabelText("Check for new mail")).toHaveValue("3"),
  );

  await fireEvent.change(screen.getByLabelText("Check for new mail"), {
    target: { value: "push" },
  });

  await waitFor(() =>
    expect(api.setNotificationSettings).toHaveBeenCalledWith({
      enabled: true,
      sound: "default",
      // Kept, not zeroed: with push on it becomes the cadence for the
      // folders IDLE does not watch.
      syncIntervalMinutes: 3,
      pushEnabled: true,
    }),
  );
});

it("picking an interval turns push back off", async () => {
  settings = {
    enabled: true,
    sound: "default",
    syncIntervalMinutes: 3,
    pushEnabled: true,
  };
  render(NotificationsPane, { props: { accounts: [] } });
  await waitFor(() =>
    expect(screen.getByLabelText("Check for new mail")).toHaveValue("push"),
  );

  await fireEvent.change(screen.getByLabelText("Check for new mail"), {
    target: { value: "5" },
  });

  await waitFor(() =>
    expect(api.setNotificationSettings).toHaveBeenCalledWith({
      enabled: true,
      sound: "default",
      syncIntervalMinutes: 5,
      pushEnabled: false,
    }),
  );
});

it("offers the other-folders cadence only while push is on", async () => {
  render(NotificationsPane, { props: { accounts: [] } });
  await waitFor(() =>
    expect(screen.getByLabelText("Check for new mail")).toHaveValue("3"),
  );
  expect(screen.queryByLabelText("Check other folders")).toBeNull();

  await fireEvent.change(screen.getByLabelText("Check for new mail"), {
    target: { value: "push" },
  });

  const other = await screen.findByLabelText("Check other folders");
  await fireEvent.change(other, { target: { value: "15" } });

  await waitFor(() =>
    expect(api.setNotificationSettings).toHaveBeenCalledWith({
      enabled: true,
      sound: "default",
      syncIntervalMinutes: 15,
      pushEnabled: true,
    }),
  );
});

it("account rows show the effective value when inheriting", async () => {
  settings = {
    enabled: false,
    sound: "default",
    syncIntervalMinutes: 3,
    pushEnabled: false,
  };
  const accounts = [
    account(1, "Work"),
    account(2, "Personal", { notifyEnabled: true, notifySound: "Ping" }),
  ];

  render(NotificationsPane, { props: { accounts } });

  // Work inherits the (off) global default; Personal overrides it.
  await waitFor(() =>
    expect(
      screen.getByLabelText("Notifications for Work"),
    ).not.toBeChecked(),
  );
  expect(screen.getByLabelText("Notifications for Personal")).toBeChecked();
  expect(screen.getByLabelText("Sound for Work")).toHaveValue("");
  expect(screen.getByLabelText("Sound for Personal")).toHaveValue("Ping");
});

it("toggling an account writes an explicit override", async () => {
  render(NotificationsPane, { props: { accounts: [account(1, "Work")] } });
  await waitFor(() =>
    expect(screen.getByLabelText("Notifications for Work")).toBeChecked(),
  );

  await fireEvent.click(screen.getByLabelText("Notifications for Work"));

  await waitFor(() =>
    expect(api.setAccountNotifications).toHaveBeenCalledWith(1, false, null),
  );
});

it("previews a picked global sound", async () => {
  render(NotificationsPane, { props: { accounts: [] } });
  await waitFor(() =>
    expect(screen.getByLabelText("Show notifications")).toBeChecked(),
  );

  await fireEvent.change(screen.getByLabelText("Sound"), {
    target: { value: "Glass" },
  });

  await waitFor(() =>
    expect(api.previewNotificationSound).toHaveBeenCalledWith("Glass"),
  );
});

it("previews a picked per-account sound, resolving Default to the global one", async () => {
  settings = {
    enabled: true,
    sound: "Glass",
    syncIntervalMinutes: 3,
    pushEnabled: false,
  };
  const accounts = [account(1, "Work", { notifySound: "Ping" })];
  render(NotificationsPane, { props: { accounts } });
  await waitFor(() =>
    expect(screen.getByLabelText("Sound for Work")).toHaveValue("Ping"),
  );

  await fireEvent.change(screen.getByLabelText("Sound for Work"), {
    target: { value: "Purr" },
  });
  await waitFor(() =>
    expect(api.previewNotificationSound).toHaveBeenCalledWith("Purr"),
  );

  // Default = inherit — previewing plays what the account will really use.
  await fireEvent.change(screen.getByLabelText("Sound for Work"), {
    target: { value: "" },
  });
  await waitFor(() =>
    expect(api.previewNotificationSound).toHaveBeenCalledWith("Glass"),
  );
});

it("picking Default clears an account's sound override", async () => {
  const accounts = [account(1, "Work", { notifySound: "Ping" })];
  render(NotificationsPane, { props: { accounts } });
  await waitFor(() =>
    expect(screen.getByLabelText("Sound for Work")).toHaveValue("Ping"),
  );

  await fireEvent.change(screen.getByLabelText("Sound for Work"), {
    target: { value: "" },
  });

  await waitFor(() =>
    expect(api.setAccountNotifications).toHaveBeenCalledWith(1, null, null),
  );
});
