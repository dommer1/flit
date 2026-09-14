import {
  cancelSummary,
  newRequestId,
  onSummaryToken,
  summarizeMessage,
  summarizeThread,
} from "./api";

/** What a summary panel shows: text grows while loading, then settles. */
export interface SummaryState {
  loading: boolean;
  text: string | null;
  error: string | null;
}

export interface SummaryRun {
  /** Stop the generation and silence further updates. */
  cancel: () => void;
  /** Settles when the run has ended one way or another. */
  done: Promise<void>;
}

/**
 * Run one summary, feeding `update` as pieces arrive, then once more with
 * the complete text (a cache hit skips straight to that). A cancelled run
 * sends no further updates — the caller has already dropped its panel.
 */
export function runSummary(
  kind: "message" | "thread",
  messageId: number,
  fresh: boolean,
  update: (state: SummaryState) => void,
): SummaryRun {
  const requestId = newRequestId();
  let text = "";
  let settled = false;
  update({ loading: true, text: null, error: null });
  const unlisten = onSummaryToken((token) => {
    if (token.requestId !== requestId || settled) return;
    text += token.text;
    update({ loading: true, text, error: null });
  });
  const call = kind === "message" ? summarizeMessage : summarizeThread;
  const done = call(messageId, fresh, requestId)
    .then((full) => {
      if (settled) return;
      settled = true;
      update({ loading: false, text: full, error: null });
    })
    .catch((err: unknown) => {
      if (settled) return;
      settled = true;
      update({ loading: false, text: null, error: String(err) });
    })
    .finally(() => {
      void unlisten.then((stop) => stop());
    });
  return {
    cancel: () => {
      if (settled) return;
      settled = true;
      void cancelSummary(requestId);
    },
    done,
  };
}
