import "./styles.css";
import { mount } from "svelte";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import App from "./App.svelte";
import SettingsWindow from "./lib/SettingsWindow.svelte";
import ComposeWindow from "./lib/ComposeWindow.svelte";
import { fetchTimingEnabled } from "./lib/api";
import { adoptBackendTiming } from "./lib/timing";

// why: every window loads this same bundle — the Tauri window label decides
// which root to mount. Plain-browser dev (`npm run dev`) has no Tauri
// context and throws here, so fall back to the main app.
function windowLabel(): string {
  try {
    return getCurrentWebviewWindow().label;
  } catch {
    return "main";
  }
}

function rootFor(label: string) {
  if (label === "settings") return SettingsWindow;
  if (label.startsWith("compose-")) return ComposeWindow;
  return App;
}

// Mirror the backend's FLIT_TIMING switch into this window.
//
// why issued before mount but not awaited: mounting must never wait on a
// debug switch, and blocking here would need top-level await, which this
// build target does not allow. The call carries no IO — it reads a bool the
// backend already resolved at startup — and it is in flight before the app's
// own first command, so in practice the flag is set before anything worth
// timing runs.
//
// why the empty catch: plain-browser dev (`npm run dev`) has no Tauri IPC,
// and there the localStorage flag still works.
void fetchTimingEnabled().then(adoptBackendTiming).catch(() => {});

const app = mount(rootFor(windowLabel()), {
  target: document.getElementById("app")!,
});

export default app;
