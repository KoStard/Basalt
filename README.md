# Basalt

Basalt is a lightweight Tauri-based Markdown viewer designed for agent-generated output.

## What it does

- Opens one or many Markdown files from the terminal.
- Accepts directories and opens all Markdown files inside (recursive).
- Renders local images referenced from Markdown.
- Opens Markdown references in a new window when clicked.
- Includes an `Open in VS Code` button.
- Supports multiple built-in themes inspired by an Obsidian-style reading experience.
- Includes a `watch` mode that opens new files as they appear.

## Terminal usage

After building, run:

```bash
basalt path/to/file.md path/to/other.md path/to/directory
```

Watch a directory and open every new Markdown file:

```bash
basalt watch path/to/directory
```

This repo also includes local launchers:

```bash
./bin/basalt path/to/file.md
./bin/watch path/to/directory
```

If you want `basalt` and `watch` globally, symlink those scripts into a directory on your `PATH`.

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

## Build

```bash
npm run tauri build
```

On macOS, the bundled app executable is inside `Basalt.app` and accepts CLI arguments.

## Notes

- Basalt can open any file path explicitly passed in the CLI, but directory scanning and watch mode target Markdown files (`.md`, `.markdown`, `.mdown`, `.mkd`, `.mdx`).
- The VS Code button requires the `code` command to be available in your shell `PATH`.
