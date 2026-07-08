---
name: workflow
description: Development workflow for this repo — test-first implementation sliced into small, reviewable, atomic commits. Use whenever implementing a feature, fix, or refactor, and whenever committing work.
---

# Workflow: test-first, small commits

The goal: a reviewer can read the branch commit-by-commit and understand each step.
Every commit is small, green, and does exactly one thing.

## The loop

1. **Slice the task** into the smallest independently-committable units.
   A unit = one behavior, one refactor, or one piece of wiring. If a unit's diff
   would exceed ~150 lines (excluding lockfiles/generated code), slice it smaller.
2. For each unit:
   a. **Write the test first.** Watch it fail for the right reason.
      - Rust logic → `#[cfg(test)]` unit test next to the code, or `src-tauri/tests/` for integration.
      - Frontend logic → Vitest test in `*.test.ts` next to the module.
      - Svelte components → Testing Library render test, only for meaningful behavior (not markup snapshots).
   b. **Implement** the minimum that makes the test pass.
   c. **Verify** — all gates for the code you touched must pass:
      - Frontend touched: `npm run check && npm test`
      - Rust touched: `cd src-tauri && cargo fmt && cargo clippy -- -D warnings && cargo test`
   d. **Commit** (see format below). Test + implementation for the same unit go
      in the same commit, so every commit is green on its own.
3. Repeat until the task is done. Then review the whole branch:
   `git log --oneline origin/main..` — the sequence should read like a story.

## Commit format

Conventional Commits, imperative mood, lowercase:

```
<type>(<scope>): <what changed>

<why — only if not obvious from the diff>
```

- **type:** `feat`, `fix`, `refactor`, `test`, `chore`, `docs`, `perf`, `build`
- **scope:** the area — e.g. `ui`, `core`, `tauri`, `ipc`, `settings`
- Subject ≤ 72 chars. Body explains *why*, never restates the diff.

Examples of a well-sliced sequence for "add a notes feature":

```
feat(core): add note model with create/validate logic
feat(tauri): expose create_note and list_notes commands
feat(ui): add note list view backed by list_notes
feat(ui): add note creation form
test(ipc): cover command error mapping for invalid notes
```

## Hard rules

- **Never mix** a refactor with a behavior change in one commit. Refactor first
  (separate `refactor:` commit), then change behavior.
- **Never commit red.** If a gate fails, fix it before committing — don't
  "fix in the next commit".
- Generated files and lockfiles get their own `chore:` or `build:` commit when
  they'd drown out a review.
- No `git add -A` reflexes — stage exactly the files belonging to the unit
  (`git add <paths>`). Check with `git status` and `git diff --staged` before committing.
- Don't amend or rebase commits already pushed.
- If you discover an unrelated bug mid-task, note it, finish the current unit,
  fix it in its own `fix:` commit (or report it), don't fold it in.
