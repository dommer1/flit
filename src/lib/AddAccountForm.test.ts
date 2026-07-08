import { expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import AddAccountForm from "./AddAccountForm.svelte";

async function fill(label: string, value: string) {
  await fireEvent.input(screen.getByLabelText(label), { target: { value } });
}

it("submits the account payload and the password separately", async () => {
  const onSubmit = vi.fn();
  render(AddAccountForm, { props: { onSubmit, onCancel: vi.fn() } });

  await fill("Name", "Work");
  await fill("Email", "hello@vocalio.sk");
  await fill("IMAP host", "imap.vocalio.sk");
  await fill("SMTP host", "smtp.vocalio.sk");
  await fill("Username", "hello@vocalio.sk");
  await fill("Password", "s3cret");
  await fireEvent.click(screen.getByRole("button", { name: "Verify & Save" }));

  expect(onSubmit).toHaveBeenCalledWith(
    {
      name: "Work",
      email: "hello@vocalio.sk",
      imapHost: "imap.vocalio.sk",
      imapPort: 993,
      smtpHost: "smtp.vocalio.sk",
      smtpPort: 587,
      username: "hello@vocalio.sk",
    },
    "s3cret",
  );
});

it("calls onCancel when cancel is clicked", async () => {
  const onCancel = vi.fn();
  render(AddAccountForm, { props: { onSubmit: vi.fn(), onCancel } });

  await fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

  expect(onCancel).toHaveBeenCalled();
});
