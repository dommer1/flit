import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { svelteTesting } from "@testing-library/svelte/vite";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [svelte(), svelteTesting()],

  // Tauri expects a fixed port and does its own console output
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },

  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts"],
    setupFiles: ["src/test-setup.ts"],
  },
});
