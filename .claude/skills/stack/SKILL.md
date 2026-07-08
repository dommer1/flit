---
name: stack
description: Everything needed to work with the Rust + Tauri 2 + Svelte 5 + Vite stack in this repo — install/verify the toolchain, run and build the app, wire Rust↔Svelte IPC, test each layer, troubleshoot common failures. Use when setting up the environment, starting the app, adding Tauri commands, or when a build/dev command fails.
---

# Stack: Rust + Tauri 2 + Svelte 5 + Vite

## 1. Environment check (run first if anything fails)

```bash
rustc --version && cargo --version   # need stable Rust; if missing: source "$HOME/.cargo/env"
node --version                        # need Node ≥ 20
xcode-select -p                       # macOS: Xcode CLT must be installed
test -d node_modules || npm install
```

Missing pieces:

- **Rust:** `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y`
  then `source "$HOME/.cargo/env"`. Cargo lives in `~/.cargo/bin` — if commands
  aren't found in a fresh shell, source that env file.
- **Tauri CLI:** ships as npm devDependency (`@tauri-apps/cli`) — `npm install` covers it.
  Never `cargo install tauri-cli`; always go through `npm run tauri …`.
- **Xcode CLT (macOS):** `xcode-select --install`.

## 2. Run / build

| What | Command | Notes |
|---|---|---|
| Full app, hot reload | `npm run tauri dev` | Vite on port 1420 + Rust window. First run compiles all crates (minutes); later runs are fast. |
| Frontend only | `npm run dev` | Browser at localhost:1420. `invoke()` calls will fail — no Rust side. Fine for pure-UI work. |
| Release bundle | `npm run tauri build` | Output in `src-tauri/target/release/bundle/`. |

Run the dev app in the background when you need to keep working; kill it before
starting another instance (port 1420 conflicts otherwise).

## 3. Rust ↔ Svelte IPC (the core pattern)

Backend — `src-tauri/src/lib.rs` (or a module registered there):

```rust
#[tauri::command]
fn create_note(title: String) -> Result<Note, String> {
    core::create_note(&title).map_err(|e| e.to_string())
}
// …and register it:
.invoke_handler(tauri::generate_handler![create_note, other_cmd])
```

Frontend — anywhere in `src/`:

```ts
import { invoke } from "@tauri-apps/api/core";
const note = await invoke<Note>("create_note", { title });
```

Rules:
- snake_case command name in Rust ⇢ same string in `invoke`; **args map camelCase→snake_case automatically** (`{ noteTitle }` ⇢ `note_title`).
- Return `Result<T, String>` (or a serializable error type) — a rejected promise on the JS side.
- Keep commands thin: parse args → call plain Rust logic → map errors. Logic lives in testable modules, not in the command fn.
- Mirror payload types manually in `src/lib/types.ts` and keep them in sync.
- Events (backend→frontend push): `app.emit("event-name", payload)` in Rust,
  `listen("event-name", handler)` from `@tauri-apps/api/event` in TS.

## 4. Testing each layer

- **Rust logic:** `#[cfg(test)] mod tests` next to code; run `cargo test` in `src-tauri/`.
- **Rust lint gate:** `cargo clippy -- -D warnings` and `cargo fmt --check` in `src-tauri/`.
- **TS logic:** Vitest — `*.test.ts` next to the module; `npm test` (CI mode) or `npm run test:watch`.
- **Svelte components:** Testing Library (`@testing-library/svelte`) with jsdom, only for real behavior.
  The `svelteTesting()` plugin from `@testing-library/svelte/vite` must stay in `vite.config.ts` —
  without it Vitest loads the *server* build of Svelte and `render()` throws.
- **Type check:** `npm run check` (svelte-check).
- `invoke` in component tests: mock it — `vi.mock("@tauri-apps/api/core", …)`. Never spawn a real Tauri app in unit tests.

## 5. Troubleshooting

| Symptom | Fix |
|---|---|
| `cargo: command not found` | `source "$HOME/.cargo/env"` |
| Port 1420 already in use | Kill the previous dev instance: `lsof -ti:1420 \| xargs kill` |
| `invoke` fails with "not found" | Command not registered in `generate_handler![…]`, or name mismatch |
| Frontend-only `npm run dev` throws on invoke | Expected — no Rust backend in the browser; mock or run `npm run tauri dev` |
| Stale Rust build weirdness | `cd src-tauri && cargo clean` (full rebuild, slow) |
| Type errors after changing a command payload | Update the mirrored type in `src/lib/types.ts` |
| Blank window in dev | Check Vite output for compile errors; the window loads whatever Vite serves |
