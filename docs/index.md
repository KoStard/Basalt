---
layout: home
---

<p align="center">
  <img src="https://raw.githubusercontent.com/KoStard/Basalt/main/icons/icon-rock-light.svg" width="96" alt="Basalt icon">
</p>

<h1 align="center">Basalt</h1>

<p align="center">A lightweight terminal-first Markdown viewer for macOS.</p>

<p align="center">
  <a href="https://github.com/KoStard/Basalt/releases/latest/download/Basalt-v0.2.0-macos-universal.zip">
    <strong>⬇ Download for macOS (Universal)</strong>
  </a>
  &nbsp;&nbsp;·&nbsp;&nbsp;
  <a href="https://github.com/KoStard/Basalt">View on GitHub</a>
</p>

---

## What it does

- Open one or many Markdown files straight from the terminal
- Pipe content in via stdin — `cat notes.md | basalt`
- Expand directories recursively
- Watch a directory and open new files as they appear
- Render local images and Markdown links
- 9 built-in themes, switchable with `Cmd+Shift+P`
- Manage windows from the terminal (`basalt windows list / close`)

## Install

1. Download the zip above and unzip it
2. Drag **Basalt.app** to your `/Applications` folder
3. Run `./bin/install-cli` from the repo to get the `basalt` CLI command

> **First launch:** Basalt is not notarized. Right-click → Open to bypass Gatekeeper.

## Usage

```bash
# Open files
basalt notes.md report.json ./docs

# Pipe content
cat README.md | basalt

# Watch a directory
basalt watch ./output

# Manage windows
basalt windows list
basalt windows close notes.md
```

## Themes

Switch themes with `Cmd+Shift+P` (or `Ctrl+Shift+P` on non-Mac):

| Theme | Style |
|---|---|
| Obsidian Night | Dark, neutral |
| Graph Paper | Light, minimal |
| Moss Grove | Dark green |
| Arc Reactor | Dark blue |
| Foundry Steel | Industrial dark |
| HUD Crimson | Dark red |
| Helios Gold | Warm amber |
| Kanagawa Lotus | Soft dark |
| Gruvbox Light | Warm light |

## Requirements

- macOS 11+ (Intel or Apple Silicon)
- For the CLI: `basalt` binary on your `PATH`
