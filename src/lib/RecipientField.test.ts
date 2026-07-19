import { beforeEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import type { Contact } from "./types";

vi.mock("./api", () => ({
  listContacts: vi.fn(async (): Promise<Contact[]> => []),
}));

import * as api from "./api";
import RecipientField from "./RecipientField.svelte";

beforeEach(() => {
  vi.clearAllMocks();
});

it("shows a prefilled value as chips with an empty input", async () => {
  render(RecipientField, { props: { label: "To", value: "a@x.com, b@y.com" } });

  expect(await screen.findByText("a@x.com")).toBeInTheDocument();
  expect(screen.getByText("b@y.com")).toBeInTheDocument();
  expect(screen.getByLabelText("To")).toHaveValue("");
});

it("commits the typed address as a chip on comma", async () => {
  render(RecipientField, { props: { label: "To" } });
  const input = screen.getByLabelText("To") as HTMLInputElement;

  await fireEvent.input(input, { target: { value: "ann@x.com," } });

  expect(await screen.findByText("ann@x.com")).toBeInTheDocument();
  expect(input.value).toBe("");
});

it("turns a whole pasted list into chips, keeping the open tail editable", async () => {
  render(RecipientField, { props: { label: "To" } });
  const input = screen.getByLabelText("To") as HTMLInputElement;

  await fireEvent.input(input, {
    target: { value: "a@x.com, b@y.com, half-typed" },
  });

  expect(await screen.findByText("a@x.com")).toBeInTheDocument();
  expect(screen.getByText("b@y.com")).toBeInTheDocument();
  expect(input.value).toBe("half-typed");
});

it("commits the pending address on Enter without submitting the form", async () => {
  render(RecipientField, { props: { label: "To" } });
  const input = screen.getByLabelText("To") as HTMLInputElement;

  await fireEvent.input(input, { target: { value: "ann@x.com" } });
  const notPrevented = await fireEvent.keyDown(input, { key: "Enter" });

  expect(notPrevented).toBe(false);
  expect(await screen.findByText("ann@x.com")).toBeInTheDocument();
  expect(input.value).toBe("");
});

it("commits the pending address when the field loses focus", async () => {
  render(RecipientField, { props: { label: "To" } });
  const input = screen.getByLabelText("To") as HTMLInputElement;

  await fireEvent.input(input, { target: { value: "ann@x.com" } });
  await fireEvent.blur(input);

  expect(await screen.findByText("ann@x.com")).toBeInTheDocument();
  expect(input.value).toBe("");
});

it("turns a picked suggestion into a chip and offers the next lookup", async () => {
  vi.mocked(api.listContacts).mockResolvedValue([
    { name: "Ann Boe", email: "ann@x.com" },
  ]);
  render(RecipientField, { props: { label: "To" } });
  const input = screen.getByLabelText("To") as HTMLInputElement;

  await fireEvent.input(input, { target: { value: "an" } });
  await fireEvent.mouseDown(
    await screen.findByRole("option", { name: /ann@x\.com/ }),
  );

  expect(
    screen.getByRole("button", { name: "Remove ann@x.com" }),
  ).toBeInTheDocument();
  expect(input.value).toBe("");

  // The cleared input starts a fresh lookup for the next recipient.
  await fireEvent.input(input, { target: { value: "an" } });
  expect(
    await screen.findByRole("option", { name: /ann@x\.com/ }),
  ).toBeInTheDocument();
});

it("removes a chip by its remove button", async () => {
  render(RecipientField, { props: { label: "To", value: "a@x.com, b@y.com" } });

  await fireEvent.click(
    await screen.findByRole("button", { name: "Remove a@x.com" }),
  );

  expect(screen.queryByText("a@x.com")).toBeNull();
  expect(screen.getByText("b@y.com")).toBeInTheDocument();
});

it("removes the last chip with Backspace in an empty input", async () => {
  render(RecipientField, { props: { label: "To", value: "a@x.com, b@y.com" } });
  await screen.findByText("b@y.com");

  await fireEvent.keyDown(screen.getByLabelText("To"), { key: "Backspace" });

  expect(screen.queryByText("b@y.com")).toBeNull();
  expect(screen.getByText("a@x.com")).toBeInTheDocument();
});
