# AGENTS.md

This file defines repo-specific instructions for coding agents working in Basalt.

## Project Snapshot

- App type: Tauri 2 desktop app with a Vite + TypeScript frontend.
- Purpose: open/render files passed via CLI, including directory expansion, stdin piping, watch mode, and window management commands.
- Frontend runtime: `src/main.ts` + `src/styles.css`
- Theme registry: `src/themes.ts`
- Backend runtime: `src-tauri/src/lib.rs`
- Built-in themes: `obsidian`, `paper`, `grove`, `reactor`, `foundry`, `hud`, `helios`, `kanagawa`, `gruvbox`.

## Repository Map

- `src/main.ts`: Document render pipeline, link/image hydration, theme switching, command palette, Tauri event listeners.
- `src/styles.css`: base token defaults and UI styling.
- `src/themes.ts`: built-in theme metadata and token values used by theme switching.
- `src-tauri/src/lib.rs`: CLI argument handling, file discovery, watch mode, window management, Tauri commands.
- `src-tauri/tauri.conf.json`: Tauri build config. The `bundle.macOS.info` block injects `CFBundleDocumentTypes` into the generated `Info.plist`, declaring Basalt as a handler for Markdown file extensions. This is required for macOS to allow setting Basalt as the default app for `.md` files.
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

- Keep `basalt <file|dir>...` behavior stable: open files and expand directories recursively.
- Keep `basalt <file|dir>...` non-blocking from the terminal via launcher scripts unless intentionally changing UX.
- Keep `cat ... | basalt` behavior stable: accept Markdown from stdin and open it in a window.
- Keep `basalt watch <directory>` behavior stable: watch recursively and open newly seen files.
- Keep `basalt windows ...` behavior stable: list and close document windows against the running app instance.
- Preserve supported Markdown extensions in Rust for Markdown-vs-code-block rendering unless intentionally changing product behavior.
- If you add or rename a Tauri command, update both `src-tauri/src/lib.rs` and calls in `src/main.ts`.
- If link/reference behavior changes, keep `resolve_references` in Rust and hydration logic in `src/main.ts` consistent.
- Do not introduce extra frontend frameworks; keep using vanilla TypeScript + DOM APIs.
- Keep styling token-driven (`:root` variables and theme token maps) rather than one-off hard-coded colors.
- Keep theme IDs and token sets in `src/themes.ts` synchronized with any theme docs and README references.

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
