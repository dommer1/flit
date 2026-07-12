import "./styles.css";
import { mount } from "svelte";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import App from "./App.svelte";
import SettingsWindow from "./lib/SettingsWindow.svelte";
import ComposeWindow from "./lib/ComposeWindow.svelte";

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

const app = mount(rootFor(windowLabel()), {
  target: document.getElementById("app")!,
});

export default app;
