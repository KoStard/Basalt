# AGENTS.md

This file defines repo-specific instructions for coding agents working in Basalt.

## Project Snapshot

- App type: Tauri 2 desktop app with a Vite + TypeScript frontend.
- Purpose: open/render Markdown files passed via CLI, including directory expansion, watch mode, and window management commands.
- Frontend runtime: `src/main.ts` + `src/styles.css`
- Backend runtime: `src-tauri/src/lib.rs`

## Repository Map

- `src/main.ts`: Markdown render pipeline, link/image hydration, theme switching, command palette, Tauri event listeners.
- `src/styles.css`: all theme tokens and UI styling.
- `src-tauri/src/lib.rs`: CLI argument handling, file discovery, watch mode, window management, Tauri commands.
- `src-tauri/src/main.rs`: entrypoint calling `basalt_lib::run()`.
- `bin/basalt`: local launcher that prefers bundle/release/debug binaries and detaches for normal open commands.
- `bin/install-cli`: builds and installs/symlinks the `basalt` launcher into `~/.local/bin` (or custom path).

## Canonical Commands

- Install deps: `npm install`
- Frontend-only dev: `npm run dev`
- Full app dev: `npm run tauri dev`
- Build frontend: `npm run build`
- Build app bundle: `npm run tauri build`
- Local launcher usage: `./bin/basalt <paths...>`, `./bin/basalt watch <directory>`, `./bin/basalt windows list`, `./bin/basalt windows close <path>`

## Implementation Rules

- Keep `basalt <file|dir>...` behavior stable: open Markdown files and expand directories recursively.
- Keep `basalt <file|dir>...` non-blocking from the terminal via launcher scripts unless intentionally changing UX.
- Keep `basalt watch <directory>` behavior stable: watch recursively and open newly seen Markdown files.
- Keep `basalt windows ...` behavior stable: list and close document windows against the running app instance.
- Preserve supported Markdown extensions in Rust unless intentionally changing product behavior.
- If you add or rename a Tauri command, update both `src-tauri/src/lib.rs` and calls in `src/main.ts`.
- If link/reference behavior changes, keep `resolve_references` in Rust and hydration logic in `src/main.ts` consistent.
- Do not introduce extra frontend frameworks; keep using vanilla TypeScript + DOM APIs.
- Keep styling token-driven (`:root` variables and theme variants) rather than one-off hard-coded colors.

## Commit and Docs Hygiene

- Prefer iterative commits over large monolithic commits.
- Each commit message should clearly document intent and the primary change.
- Keep docs clean and consistently structured with clear headings and concise sections.
- Update affected documentation in the same change when behavior, commands, or workflows change.

## Validation Checklist

Run the most relevant checks for your change:

1. `npm run build`
2. `cargo check --manifest-path src-tauri/Cargo.toml`
3. If CLI/watch behavior changed, smoke test manually with `npm run tauri dev -- ./README.md` and `npm run tauri dev -- watch .`.

## Documentation Sync

When behavior or commands change, update `README.md` and this file in the same change set.
