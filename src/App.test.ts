import { expect, it } from "vitest";
import { render, screen } from "@testing-library/svelte";

import App from "./App.svelte";

it("renders the app heading", () => {
  render(App);

  expect(screen.getByRole("heading", { name: "Flit" })).toBeInTheDocument();
});
