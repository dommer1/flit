# Flit

Desktop app built with [Tauri 2](https://tauri.app) (Rust) + [Svelte 5](https://svelte.dev) + [Vite](https://vite.dev).

## Development

```bash
npm install          # frontend deps + Tauri CLI
npm run tauri dev    # run the desktop app with hot reload
```

Rust toolchain is required for the desktop app — install via [rustup](https://rustup.rs).

## Checks

```bash
npm run check        # svelte-check (types)
npm test             # Vitest (frontend tests)
cd src-tauri
cargo test           # Rust tests
cargo clippy -- -D warnings
cargo fmt --check
```

## Build

```bash
npm run tauri build  # release bundle in src-tauri/target/release/bundle/
```

## Layout

- `src/` — Svelte frontend (logic in `src/lib`, components thin)
- `src-tauri/` — Rust backend; Tauri commands in `src-tauri/src/lib.rs`
- `CLAUDE.md` + `.claude/skills/` — project instructions and workflows for AI agents
