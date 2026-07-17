import { expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import type { Account, Alias } from "./types";
import AccountsPane from "./AccountsPane.svelte";

const accounts: Account[] = [
  {
    id: 1,
    name: "Personal",
    email: "domco@example.com",
    imapHost: "imap.example.com",
    imapPort: 993,
    smtpHost: "smtp.example.com",
    smtpPort: 587,
    username: "domco@example.com",
    lastError: null,
    checkedAt: 1751900000,
    color: null,
    signatureId: null,
    notifyEnabled: null,
    notifySound: null,
    defaultAliasId: null,
  },
  {
    id: 2,
    name: "Work",
    email: "hello@vocalio.sk",
    imapHost: "imap.vocalio.sk",
    imapPort: 993,
    smtpHost: "smtp.vocalio.sk",
    smtpPort: 587,
    username: "hello@vocalio.sk",
    lastError: null,
    checkedAt: 1751900000,
    color: null,
    signatureId: null,
    notifyEnabled: null,
    notifySound: null,
    defaultAliasId: null,
  },
];

const aliases: Alias[] = [
  { id: 10, accountId: 2, name: "Igor", email: "igor@vocalio.sk" },
  { id: 11, accountId: 2, name: "", email: "dominik@vocalio.sk" },
  { id: 12, accountId: 1, name: "Other", email: "other@example.com" },
];

function renderPane(overrides: Record<string, unknown> = {}) {
  const props = {
    accounts,
    aliases: [] as Alias[],
    onAdd: vi.fn(async () => null),
    onDelete: vi.fn(),
    onSetColor: vi.fn(),
    onAddAlias: vi.fn(async () => null),
    onUpdateAlias: vi.fn(),
    onDeleteAlias: vi.fn(),
    onSetDefaultAlias: vi.fn(),
    ...overrides,
  };
  render(AccountsPane, { props });
  return props;
}

// why: every field is `required` and jsdom enforces constraint validation —
// a partially filled form never fires submit.
async function fillAccountForm() {
  await fireEvent.click(screen.getByLabelText("Add account"));
  await fireEvent.input(screen.getByLabelText("Name"), {
    target: { value: "New" },
  });
  await fireEvent.input(screen.getByLabelText("Email"), {
    target: { value: "new@example.com" },
  });
  await fireEvent.input(screen.getByLabelText("IMAP host"), {
    target: { value: "imap.new.com" },
  });
  await fireEvent.input(screen.getByLabelText("SMTP host"), {
    target: { value: "smtp.new.com" },
  });
  await fireEvent.input(screen.getByLabelText("Username"), {
    target: { value: "new@example.com" },
  });
  await fireEvent.input(screen.getByLabelText("Password"), {
    target: { value: "pw" },
  });
  await fireEvent.click(screen.getByRole("button", { name: "Verify & Save" }));
}

it("shows the first account's details by default", () => {
  renderPane();

  expect(screen.getByText("imap.example.com:993")).toBeInTheDocument();
  expect(screen.getByText("smtp.example.com:587")).toBeInTheDocument();
});

it("switches the detail pane when another account is selected", async () => {
  renderPane();

  await fireEvent.click(screen.getByText("Work"));

  expect(screen.getByText("imap.vocalio.sk:993")).toBeInTheDocument();
});

it("hands the selected account to onDelete", async () => {
  const { onDelete } = renderPane();

  await fireEvent.click(screen.getByText("Work"));
  await fireEvent.click(screen.getByLabelText("Delete account"));

  expect(onDelete).toHaveBeenCalledWith(accounts[1]);
});

it("submits a new account and closes the form on success", async () => {
  const onAdd = vi.fn(async () => accounts[0]);
  renderPane({ onAdd });

  await fillAccountForm();

  expect(onAdd).toHaveBeenCalledWith(
    expect.objectContaining({ name: "New", imapHost: "imap.new.com" }),
    "pw",
  );
  expect(
    screen.queryByRole("button", { name: "Verify & Save" }),
  ).not.toBeInTheDocument();
});

it("keeps the form open when adding fails", async () => {
  const { onAdd } = renderPane(); // default onAdd resolves to null = failure

  await fillAccountForm();

  expect(onAdd).toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "Verify & Save" })).toBeInTheDocument();
});

it("shows connected status for a healthy account", () => {
  renderPane();

  expect(screen.getByText("Connected")).toBeInTheDocument();
  expect(screen.queryByTitle("Connection problem")).not.toBeInTheDocument();
});

it("shows the error and a list warning for a broken account", async () => {
  const broken = {
    ...accounts[1],
    lastError: "imap error: login: denied",
    checkedAt: 1751990000,
  };
  renderPane({ accounts: [accounts[0], broken] });

  // warning marker in the list, visible without selecting the account
  expect(screen.getByTitle("Connection problem")).toBeInTheDocument();

  await fireEvent.click(screen.getByText("Work"));

  expect(screen.getByText("imap error: login: denied")).toBeInTheDocument();
  expect(screen.queryByText("Connected")).not.toBeInTheDocument();
});

it("shows not-checked-yet for a brand new account", () => {
  const fresh = { ...accounts[0], lastError: null, checkedAt: null };
  renderPane({ accounts: [fresh] });

  expect(screen.getByText("Not checked yet")).toBeInTheDocument();
});

it("shows an empty state and disables delete without accounts", () => {
  renderPane({ accounts: [] });

  expect(screen.getByText(/No accounts yet/)).toBeInTheDocument();
  expect(screen.getByLabelText("Delete account")).toBeDisabled();
});

it("sets an account color from the palette", async () => {
  const props = renderPane();

  await fireEvent.click(screen.getByLabelText("Orange"));
  expect(props.onSetColor).toHaveBeenCalledWith(1, "#ff9f0a");
});

it("clears the color with the no-color swatch", async () => {
  const colored = { ...accounts[0], color: "#ff9f0a" };
  const props = renderPane({ accounts: [colored] });

  await fireEvent.click(screen.getByLabelText("No color"));
  expect(props.onSetColor).toHaveBeenCalledWith(1, null);
});

it("lists only the selected account's aliases", async () => {
  renderPane({ aliases });

  await fireEvent.click(screen.getByText("Work"));

  expect(screen.getByDisplayValue("igor@vocalio.sk")).toBeInTheDocument();
  expect(screen.getByDisplayValue("dominik@vocalio.sk")).toBeInTheDocument();
  // the other account's alias stays out of this list
  expect(
    screen.queryByDisplayValue("other@example.com"),
  ).not.toBeInTheDocument();
});

it("adds an alias through the inline row and closes it on success", async () => {
  const onAddAlias = vi.fn(async () => aliases[0]);
  renderPane({ aliases, onAddAlias });

  await fireEvent.click(screen.getByText("Work"));
  await fireEvent.click(screen.getByText("+ Add alias…"));
  await fireEvent.input(screen.getByLabelText("New alias name"), {
    target: { value: "Support" },
  });
  await fireEvent.input(screen.getByLabelText("New alias address"), {
    target: { value: "support@vocalio.sk" },
  });
  await fireEvent.click(screen.getByRole("button", { name: "Add" }));

  expect(onAddAlias).toHaveBeenCalledWith(2, "Support", "support@vocalio.sk");
  expect(screen.queryByLabelText("New alias address")).not.toBeInTheDocument();
});

it("keeps the inline row open when adding fails", async () => {
  const { onAddAlias } = renderPane({ aliases }); // resolves null = failure

  await fireEvent.click(screen.getByText("+ Add alias…"));
  await fireEvent.input(screen.getByLabelText("New alias address"), {
    target: { value: "support@example.com" },
  });
  await fireEvent.click(screen.getByRole("button", { name: "Add" }));

  expect(onAddAlias).toHaveBeenCalled();
  expect(screen.getByLabelText("New alias address")).toBeInTheDocument();
});

it("edits an alias address in place", async () => {
  const { onUpdateAlias } = renderPane({ aliases });

  await fireEvent.click(screen.getByText("Work"));
  await fireEvent.change(screen.getByDisplayValue("igor@vocalio.sk"), {
    target: { value: "igor2@vocalio.sk" },
  });

  expect(onUpdateAlias).toHaveBeenCalledWith(10, "Igor", "igor2@vocalio.sk");
});

it("deletes an alias from its row", async () => {
  const { onDeleteAlias } = renderPane({ aliases });

  await fireEvent.click(screen.getByText("Work"));
  await fireEvent.click(
    screen.getByLabelText("Delete alias igor@vocalio.sk"),
  );

  expect(onDeleteAlias).toHaveBeenCalledWith(10);
});

it("marks an alias as the default identity", async () => {
  const { onSetDefaultAlias } = renderPane({ aliases });

  await fireEvent.click(screen.getByText("Work"));
  await fireEvent.click(
    screen.getByLabelText("Make igor@vocalio.sk the default"),
  );

  expect(onSetDefaultAlias).toHaveBeenCalledWith(2, 10);
});

it("returns the default identity to the account's own address", async () => {
  const withDefault = [
    accounts[0],
    { ...accounts[1], defaultAliasId: 10 },
  ];
  const { onSetDefaultAlias } = renderPane({ accounts: withDefault, aliases });

  await fireEvent.click(screen.getByText("Work"));
  await fireEvent.click(
    screen.getByLabelText("Make hello@vocalio.sk the default"),
  );

  expect(onSetDefaultAlias).toHaveBeenCalledWith(2, null);
});
