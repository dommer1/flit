import "./styles.css";
import { mount } from "svelte";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import App from "./App.svelte";
import SettingsWindow from "./lib/SettingsWindow.svelte";

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

const app = mount(windowLabel() === "settings" ? SettingsWindow : App, {
  target: document.getElementById("app")!,
});

export default app;
