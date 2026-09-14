import { fireEvent, render, screen, within } from "@testing-library/svelte";
import { beforeEach, expect, it, vi } from "vitest";
import type { LlmStatus } from "./types";

let enabled = false;
let status: LlmStatus;

vi.mock("./api", () => ({
  getLlmSummaryEnabled: vi.fn(async () => enabled),
  setLlmSummaryEnabled: vi.fn(async () => undefined),
  llmStatus: vi.fn(async () => status),
  setLlmModel: vi.fn(async () => undefined),
}));

import * as api from "./api";
import ExperimentalPane from "./ExperimentalPane.svelte";

beforeEach(() => {
  vi.clearAllMocks();
  enabled = false;
  status = {
    enabled: false,
    activeModel: null,
    ready: false,
    models: [
      {
        id: "small",
        name: "Small 2B",
        description: "Fast and small.",
        size: 1_280_835_840,
        recommended: true,
        state: { kind: "missing" },
      },
      {
        id: "big",
        name: "Big 4B",
        description: "Better summaries.",
        size: 2_740_937_888,
        recommended: false,
        state: { kind: "ready" },
      },
    ],
  };
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

it("keeps the model picker hidden while the feature is off", async () => {
  render(ExperimentalPane);
  await screen.findByLabelText("Summarize messages with a local AI model");

  expect(screen.queryByText("Small 2B")).toBeNull();
});

it("lists the catalog with sizes and states once switched on", async () => {
  enabled = true;
  status.enabled = true;
  render(ExperimentalPane);

  const small = (await screen.findByText("Small 2B")).closest("li")!;
  expect(within(small).getByText("1.3 GB")).toBeInTheDocument();
  expect(within(small).getByText("Recommended")).toBeInTheDocument();
  expect(within(small).getByText("Not downloaded")).toBeInTheDocument();

  const big = screen.getByText("Big 4B").closest("li")!;
  expect(within(big).getByText("2.7 GB")).toBeInTheDocument();
  expect(within(big).getByText("Downloaded")).toBeInTheDocument();
});

it("lets only a downloaded model be picked as the one in use", async () => {
  enabled = true;
  status.enabled = true;
  render(ExperimentalPane);
  await screen.findByText("Small 2B");

  expect(screen.getByLabelText("Use Small 2B")).toBeDisabled();
  const big = screen.getByLabelText("Use Big 4B");
  expect(big).toBeEnabled();

  await fireEvent.click(big);

  expect(api.setLlmModel).toHaveBeenCalledWith("big");
});

it("marks the picked model as in use", async () => {
  enabled = true;
  status.enabled = true;
  status.activeModel = "big";
  render(ExperimentalPane);

  expect(await screen.findByLabelText("Use Big 4B")).toBeChecked();
});
