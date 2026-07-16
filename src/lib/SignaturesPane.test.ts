import { beforeEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import type { Account, Signature } from "./types";

function account(id: number, email: string, signatureId: number | null): Account {
  return {
    id,
    name: email,
    email,
    imapHost: "imap.example.com",
    imapPort: 993,
    smtpHost: "smtp.example.com",
    smtpPort: 587,
    username: email,
    lastError: null,
    checkedAt: null,
    color: null,
    signatureId,
  };
}

let signatures: Signature[] = [];

vi.mock("./api", () => ({
  listSignatures: vi.fn(async () => signatures),
  createSignature: vi.fn(async (name: string) => ({
    id: 99,
    name,
    body: "",
  })),
  updateSignature: vi.fn(async () => undefined),
  deleteSignature: vi.fn(async () => undefined),
  setSignatureAccounts: vi.fn(async () => undefined),
  onSignaturesChanged: vi.fn(async () => () => {}),
}));

import * as api from "./api";
import SignaturesPane from "./SignaturesPane.svelte";

const accounts = [
  account(1, "a@example.com", null),
  account(2, "b@example.com", 10),
];

beforeEach(() => {
  vi.clearAllMocks();
  signatures = [
    { id: 10, name: "Vocalio", body: "<p>S pozdravom</p>" },
    { id: 11, name: "Personal", body: "<p>Dominik</p>" },
  ];
});

it("lists signatures and opens the first one in the editor", async () => {
  render(SignaturesPane, { props: { accounts } });

  expect(await screen.findByRole("button", { name: "Vocalio" })).toHaveClass(
    "selected",
  );
  expect(screen.getByRole("button", { name: "Personal" })).not.toHaveClass(
    "selected",
  );
  expect(screen.getByLabelText("Signature name")).toHaveValue("Vocalio");
  expect(
    await screen.findByRole("textbox", { name: "Signature body" }),
  ).toHaveTextContent("S pozdravom");
});

it("saves the edited name and body", async () => {
  render(SignaturesPane, { props: { accounts } });
  await screen.findByRole("textbox", { name: "Signature body" });

  await fireEvent.input(screen.getByLabelText("Signature name"), {
    target: { value: "Work" },
  });
  await fireEvent.click(screen.getByRole("button", { name: "Save" }));

  await waitFor(() =>
    expect(api.updateSignature).toHaveBeenCalledWith(
      10,
      "Work",
      expect.stringContaining("S pozdravom"),
    ),
  );
});

it("creates a signature via + and selects it", async () => {
  render(SignaturesPane, { props: { accounts } });
  await screen.findByRole("button", { name: "Vocalio" });

  signatures = [...signatures, { id: 99, name: "Signature", body: "" }];
  await fireEvent.click(screen.getByRole("button", { name: "Add signature" }));

  await waitFor(() => expect(api.createSignature).toHaveBeenCalledWith("Signature"));
  expect(
    await screen.findByRole("button", { name: "Signature" }),
  ).toHaveClass("selected");
});

it("deletes the selected signature", async () => {
  render(SignaturesPane, { props: { accounts } });
  await screen.findByRole("button", { name: "Vocalio" });

  signatures = signatures.filter((sig) => sig.id !== 10);
  await fireEvent.click(
    screen.getByRole("button", { name: "Delete signature" }),
  );

  await waitFor(() => expect(api.deleteSignature).toHaveBeenCalledWith(10));
  // The next remaining signature takes over the editor.
  expect(
    await screen.findByRole("button", { name: "Personal" }),
  ).toHaveClass("selected");
});

it("toggles one account as default for the signature", async () => {
  render(SignaturesPane, { props: { accounts } });
  await screen.findByRole("textbox", { name: "Signature body" });

  // Account 2 already defaults to signature 10; adding account 1 keeps it.
  await fireEvent.click(screen.getByLabelText("a@example.com"));

  await waitFor(() =>
    expect(api.setSignatureAccounts).toHaveBeenCalledWith(
      10,
      expect.arrayContaining([1, 2]),
    ),
  );
});

it("claims all accounts via the All checkbox", async () => {
  render(SignaturesPane, { props: { accounts } });
  await screen.findByRole("textbox", { name: "Signature body" });

  await fireEvent.click(screen.getByLabelText("All"));

  await waitFor(() =>
    expect(api.setSignatureAccounts).toHaveBeenCalledWith(10, [1, 2]),
  );
});
