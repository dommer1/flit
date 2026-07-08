import { expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (_cmd: string, args: { name: string }) => `Hello, ${args.name}!`),
}));

import App from "./App.svelte";

it("renders the app heading", () => {
  render(App);

  expect(screen.getByRole("heading", { name: "Flit" })).toBeInTheDocument();
});

it("shows no greeting before the first submit", () => {
  render(App);

  expect(screen.queryByText(/Hello/)).not.toBeInTheDocument();
});

it("greets via the greet command on submit", async () => {
  render(App);

  const input = screen.getByPlaceholderText("Enter a name…");
  await fireEvent.input(input, { target: { value: "Domco" } });
  await fireEvent.click(screen.getByRole("button", { name: "Greet" }));

  expect(await screen.findByText("Hello, Domco!")).toBeInTheDocument();
});
