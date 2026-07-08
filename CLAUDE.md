# Flit

Desktop app built with Tauri 2 (Rust backend) + Svelte 5 (TypeScript frontend) + Vite.

## Stack

- **Backend:** Rust, Tauri 2 — lives in `src-tauri/`
- **Frontend:** Svelte 5 + TypeScript + Vite — lives in `src/`
- **Frontend tests:** Vitest (+ Testing Library for components)
- **Rust tests:** built-in `cargo test`
- **Package manager:** npm

## Commands

| Task | Command |
|---|---|
| Run the desktop app (dev) | `npm run tauri dev` |
| Run frontend only in browser | `npm run dev` |
| Build release app | `npm run tauri build` |
| Frontend tests | `npm test` |
| Frontend type/lint check | `npm run check` |
| Rust tests | `cargo test` (run in `src-tauri/`) |
| Rust lint | `cargo clippy -- -D warnings` (in `src-tauri/`) |
| Rust format | `cargo fmt` (in `src-tauri/`) |

## Project structure

```
src/                  Svelte frontend (components, lib, stores)
src/lib/              Shared frontend code — put logic here, keep components thin
src-tauri/src/        Rust code; Tauri commands in lib.rs (or modules it declares)
src-tauri/tauri.conf.json  Tauri app config (window, bundle, dev server)
```

## Rules

- **Always follow the `workflow` skill** when implementing anything: test-first,
  small atomic commits, every commit green. Never batch a whole feature into one commit.
- **Use the `stack` skill** for environment setup, running the app, Rust↔Svelte IPC
  patterns, and troubleshooting.
- Business logic goes in plain, testable units (Rust modules / `src/lib` TS modules),
  not inside UI components or command handlers — those stay thin wrappers.
- Tauri commands are the only bridge between frontend and backend. Keep their
  payloads small, serializable, and typed on both sides.
- Prefer `npm run check` + `npm test` + `cargo test` + `cargo clippy` all passing
  before any commit.
