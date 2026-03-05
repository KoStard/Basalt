# Basalt

Basalt is a lightweight Tauri-based document viewer designed for agent-generated output.

## What it does

- Opens one or many files from the terminal.
- Accepts directories and opens all files inside (recursive).
- Renders Markdown files as rich content.
- Renders non-Markdown files as code blocks.
- Accepts piped Markdown from standard input (for example, `cat notes.md | basalt`).
- Renders local images referenced from Markdown.
- Opens Markdown references in a new window when clicked.
- Includes an `Open in VS Code` button.
- Supports nine built-in themes: Obsidian Night, Graph Paper, Moss Grove, Arc Reactor, Foundry Steel, HUD Crimson, Helios Gold, Kanagawa Lotus, and Gruvbox Light (switch via `Ctrl/Cmd+Shift+P`).
- Includes a `watch` mode that opens new files as they appear.

## Terminal usage

After building, run:

```bash
basalt path/to/file.md path/to/config.json path/to/directory
```

Pipe Markdown directly into Basalt:

```bash
cat path/to/file.md | basalt
```

`basalt` launches the app and then immediately returns control to the terminal.

Watch a directory and open every new file:

```bash
basalt watch path/to/directory
```

List currently open Basalt document windows:

```bash
basalt windows list
basalt windows list --json
```

Close windows from the terminal:

```bash
basalt windows close path/to/file.md
basalt windows close --label doc-3
```

You can also run the local launcher directly:

```bash
./bin/basalt path/to/file.md
./bin/basalt watch path/to/directory
```

## Install as a CLI

Install globally by compiling and copying the binary:

```bash
./bin/install-cli
```

By default this installs `basalt` into `~/.local/bin`.  
Optional custom install path:

```bash
./bin/install-cli /your/path/on/PATH
```

The installer runs `npm run tauri build` and links the Basalt launcher.

## Development

```bash
npm install
npm run tauri dev
```

Pass startup paths while running in dev mode:

```bash
npm run tauri dev -- ./notes.md ./reports
```

Run watch mode in dev:

```bash
npm run tauri dev -- watch ./reports
```

## Add a theme

Theme definitions are centralized in `src/themes.ts`.

1. Add a new object to the `THEMES` array with a unique `id`, `label`, and search `keywords`.
2. Fill in all `tokens` keys: `surface`, `surface-alt`, `panel`, `text`, `muted`, `accent`, `accent-strong`, `line`, `code-bg`, `quote`, `button-bg`, `button-hover`, `status-bg`.
3. Run `npm run build` to validate TypeScript and bundling.
4. Launch Basalt and switch via `Ctrl/Cmd+Shift+P` to confirm the new theme appears and renders correctly.

## Build

```bash
npm run tauri build
```

On macOS, the bundled app executable is inside `Basalt.app` and accepts CLI arguments.

## Notes

- Markdown files (`.md`, `.markdown`, `.mdown`, `.mkd`, `.mdx`) render as Markdown; other files render as code blocks.
- Piped stdin content is saved to a temporary Markdown file before opening so it works with single-instance forwarding.
- The VS Code button requires the `code` command to be available in your shell `PATH`.
