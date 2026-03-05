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
- Supports seven built-in themes: Obsidian Night, Graph Paper, Moss Grove, Arc Reactor, Foundry Steel, HUD Crimson, and Helios Gold (switch via `Ctrl/Cmd+Shift+P`).
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

Watch a directory and open every new file:

```bash
basalt watch path/to/directory
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

The installer runs `npm run build`, compiles a release binary with Cargo, and installs it into your target directory.

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

- Markdown files (`.md`, `.markdown`, `.mdown`, `.mkd`, `.mdx`) render as Markdown; other files render as code blocks.
- Piped stdin content is saved to a temporary Markdown file before opening so it works with single-instance forwarding.
- The VS Code button requires the `code` command to be available in your shell `PATH`.
