import { fireEvent, render, screen } from "@testing-library/svelte";
import { beforeEach, expect, it, vi } from "vitest";

let enabled = false;

vi.mock("./api", () => ({
  getLlmSummaryEnabled: vi.fn(async () => enabled),
  setLlmSummaryEnabled: vi.fn(async () => undefined),
}));

import * as api from "./api";
import ExperimentalPane from "./ExperimentalPane.svelte";

beforeEach(() => {
  vi.clearAllMocks();
  enabled = false;
});

it("shows on-device summaries switched off by default", async () => {
  render(ExperimentalPane);

  expect(
    await screen.findByLabelText("Summarize messages with a local AI model"),
  ).not.toBeChecked();
});

it("saves switching on-device summaries on", async () => {
  render(ExperimentalPane);
  const box = await screen.findByLabelText(
    "Summarize messages with a local AI model",
  );

  await fireEvent.click(box);

  expect(api.setLlmSummaryEnabled).toHaveBeenCalledWith(true);
});

it("rolls the switch back when saving fails", async () => {
  vi.mocked(api.setLlmSummaryEnabled).mockRejectedValueOnce(new Error("disk"));
  render(ExperimentalPane);
  const box = await screen.findByLabelText(
    "Summarize messages with a local AI model",
  );

  await fireEvent.click(box);

  expect(await screen.findByRole("alert")).toHaveTextContent("disk");
  expect(box).not.toBeChecked();
});
