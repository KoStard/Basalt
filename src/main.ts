import DOMPurify from "dompurify";
import { marked } from "marked";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";

type LoadedDocument = {
  path: string;
  fileName: string;
  markdown: string;
};

type ResolvedReferenceMap = Record<string, string | null>;

type PaletteCommand = {
  id: string;
  label: string;
  keywords: string;
  run: () => Promise<void> | void;
};

const THEMES = [
  { id: "obsidian", label: "Obsidian Night", keywords: "dark obsidian night" },
  { id: "paper", label: "Graph Paper", keywords: "light paper graph" },
  { id: "grove", label: "Moss Grove", keywords: "green grove moss" },
  { id: "reactor", label: "Arc Reactor", keywords: "neon blue stark tech futuristic" },
  { id: "foundry", label: "Foundry Steel", keywords: "industrial graphite steel workshop amber" },
  { id: "hud", label: "HUD Crimson", keywords: "red cockpit visor tactical command" },
  { id: "helios", label: "Helios Gold", keywords: "light gold titanium luxury bright" },
] as const;

type ThemeId = (typeof THEMES)[number]["id"];

const THEME_STORAGE_KEY = "basalt.theme";
const THEME_IDS = new Set<ThemeId>(THEMES.map((theme) => theme.id));

const viewerEl = document.querySelector<HTMLElement>("#viewer");
const statusTextEl = document.querySelector<HTMLElement>("#status-text");
const pathEl = document.querySelector<HTMLElement>("#doc-path");
const vscodeBtn = document.querySelector<HTMLButtonElement>("#vscode-btn");
const commandPaletteEl = document.querySelector<HTMLElement>("#command-palette");
const commandInputEl = document.querySelector<HTMLInputElement>("#command-input");
const commandResultsEl = document.querySelector<HTMLUListElement>("#command-results");

let commandList: PaletteCommand[] = [];
let filteredCommands: PaletteCommand[] = [];
let selectedCommandIndex = 0;
let isPaletteOpen = false;

marked.setOptions({ gfm: true, breaks: false });

function setStatus(message: string, isError = false): void {
  if (!statusTextEl) {
    return;
  }
  statusTextEl.textContent = message;
  statusTextEl.dataset.tone = isError ? "error" : "neutral";
}

function currentThemeLabel(theme: ThemeId): string {
  return THEMES.find((entry) => entry.id === theme)?.label ?? theme;
}

function applyTheme(theme: ThemeId): void {
  document.documentElement.dataset.theme = theme;
  localStorage.setItem(THEME_STORAGE_KEY, theme);
}

function restoreTheme(): void {
  const stored = localStorage.getItem(THEME_STORAGE_KEY);
  const nextTheme: ThemeId = stored && THEME_IDS.has(stored as ThemeId) ? (stored as ThemeId) : "obsidian";
  applyTheme(nextTheme);
}

function hasUrlScheme(value: string): boolean {
  if (value.startsWith("//")) {
    return true;
  }
  return /^[a-zA-Z][a-zA-Z\d+.-]*:/.test(value);
}

function isExternalReference(reference: string): boolean {
  if (/^[a-zA-Z]:\\/.test(reference)) {
    return false;
  }
  return hasUrlScheme(reference);
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/\"/g, "&quot;")
    .replace(/'/g, "&#039;");
}

function renderEmptyState(message?: string): void {
  if (!viewerEl) {
    return;
  }

  const details = message
    ? `<p class="empty-error">${escapeHtml(message)}</p>`
    : "<p>Launch Basalt from your terminal with one or more Markdown files.</p>";

  viewerEl.innerHTML = `
    <section class="empty-state">
      <h1>Basalt is waiting for a document</h1>
      ${details}
      <pre><code>basalt ./notes/today.md ./reports ./summary.md</code></pre>
      <pre><code>basalt watch ./reports</code></pre>
    </section>
  `;

  if (pathEl) {
    pathEl.textContent = "Open via terminal to get started.";
    pathEl.title = "Open via terminal to get started.";
  }
}

async function resolveReferences(): Promise<ResolvedReferenceMap> {
  if (!viewerEl) {
    return {};
  }

  const references = new Set<string>();

  viewerEl.querySelectorAll<HTMLImageElement>("img[src]").forEach((node) => {
    const source = node.getAttribute("src")?.trim();
    if (!source || source.startsWith("data:") || isExternalReference(source)) {
      return;
    }
    references.add(source);
  });

  viewerEl.querySelectorAll<HTMLAnchorElement>("a[href]").forEach((node) => {
    const href = node.getAttribute("href")?.trim();
    if (!href || href.startsWith("#") || isExternalReference(href)) {
      return;
    }
    references.add(href);
  });

  if (!references.size) {
    return {};
  }

  return invoke<ResolvedReferenceMap>("resolve_references", {
    references: [...references],
  });
}

async function hydrateReferences(): Promise<void> {
  if (!viewerEl) {
    return;
  }

  const resolved = await resolveReferences();

  viewerEl.querySelectorAll<HTMLImageElement>("img[src]").forEach((node) => {
    const source = node.getAttribute("src")?.trim();
    if (!source) {
      return;
    }

    if (source.startsWith("data:")) {
      return;
    }

    if (isExternalReference(source)) {
      node.loading = "lazy";
      return;
    }

    const resolvedPath = resolved[source];
    if (resolvedPath) {
      node.src = convertFileSrc(resolvedPath);
      node.loading = "lazy";
    } else {
      node.classList.add("broken-resource");
      node.title = `Missing resource: ${source}`;
    }
  });

  viewerEl.querySelectorAll<HTMLAnchorElement>("a[href]").forEach((node) => {
    const href = node.getAttribute("href")?.trim();
    if (!href || href.startsWith("#")) {
      return;
    }

    if (isExternalReference(href)) {
      node.classList.add("external-link");
      node.target = "_blank";
      node.rel = "noreferrer noopener";
      return;
    }

    if (resolved[href]) {
      node.classList.add("internal-link");
    } else {
      node.classList.add("broken-link");
      node.title = `Missing target: ${href}`;
    }
  });
}

async function renderDocument(document: LoadedDocument): Promise<void> {
  if (!viewerEl) {
    return;
  }

  if (pathEl) {
    pathEl.textContent = document.path;
    pathEl.title = document.path;
  }

  const rendered = marked.parse(document.markdown, { async: false }) as string;
  const sanitized = DOMPurify.sanitize(rendered);
  viewerEl.innerHTML = sanitized;

  await hydrateReferences();
}

async function loadDocument(reason: string): Promise<void> {
  try {
    const document = await invoke<LoadedDocument>("load_document");
    await renderDocument(document);
    setStatus(reason);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    renderEmptyState(message);
    setStatus("No document assigned to this window.", true);
  }
}

async function openCurrentFileInVSCode(): Promise<void> {
  try {
    await invoke("open_in_vscode");
    setStatus("Opened current file in VS Code.");
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    setStatus(`Unable to open VS Code: ${message}`, true);
  }
}

async function handleLinkClick(href: string): Promise<void> {
  if (href.startsWith("#")) {
    return;
  }

  if (isExternalReference(href)) {
    try {
      await openUrl(href);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setStatus(`Unable to open external link: ${message}`, true);
    }
    return;
  }

  try {
    await invoke("open_reference", { reference: href });
    setStatus(`Opened reference: ${href}`);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    setStatus(`Unable to open reference: ${message}`, true);
  }
}

function fuzzyScore(query: string, candidate: string): number | null {
  const q = query.trim().toLowerCase();
  if (!q) {
    return 0;
  }

  const source = candidate.toLowerCase();
  let qIndex = 0;
  let score = 0;
  let streak = 0;

  for (let i = 0; i < source.length && qIndex < q.length; i += 1) {
    if (source[i] !== q[qIndex]) {
      streak = 0;
      continue;
    }

    score += 10;
    if (i < 12) {
      score += 3;
    }

    streak += 1;
    if (streak > 1) {
      score += streak * 2;
    }

    qIndex += 1;
  }

  if (qIndex !== q.length) {
    return null;
  }

  return score - (source.length - q.length);
}

function matchingCommands(query: string): PaletteCommand[] {
  const scored = commandList
    .map((command) => {
      const score = fuzzyScore(query, `${command.label} ${command.keywords}`);
      return score === null ? null : { command, score };
    })
    .filter((entry): entry is { command: PaletteCommand; score: number } => entry !== null)
    .sort((left, right) => right.score - left.score || left.command.label.localeCompare(right.command.label));

  return scored.map((entry) => entry.command);
}

function renderCommandResults(): void {
  if (!commandResultsEl || !commandInputEl) {
    return;
  }

  filteredCommands = matchingCommands(commandInputEl.value);

  if (filteredCommands.length === 0) {
    selectedCommandIndex = 0;
    commandResultsEl.innerHTML = '<li class="command-empty">No matching commands.</li>';
    return;
  }

  if (selectedCommandIndex >= filteredCommands.length) {
    selectedCommandIndex = 0;
  }

  commandResultsEl.innerHTML = "";

  filteredCommands.forEach((command, index) => {
    const item = document.createElement("li");
    const button = document.createElement("button");
    button.type = "button";
    button.className = "command-item";

    if (index === selectedCommandIndex) {
      button.classList.add("is-selected");
    }

    button.textContent = command.label;
    button.dataset.index = String(index);

    button.addEventListener("mouseenter", () => {
      selectedCommandIndex = index;
      renderCommandResults();
    });

    button.addEventListener("click", () => {
      void runCommandByIndex(index);
    });

    item.appendChild(button);
    commandResultsEl.appendChild(item);
  });
}

function openCommandPalette(): void {
  if (!commandPaletteEl || !commandInputEl) {
    return;
  }

  commandPaletteEl.hidden = false;
  commandPaletteEl.setAttribute("aria-hidden", "false");
  isPaletteOpen = true;

  commandInputEl.value = "";
  selectedCommandIndex = 0;
  renderCommandResults();

  window.requestAnimationFrame(() => {
    commandInputEl.focus();
    commandInputEl.select();
  });
}

function closeCommandPalette(): void {
  if (!commandPaletteEl) {
    return;
  }

  commandPaletteEl.hidden = true;
  commandPaletteEl.setAttribute("aria-hidden", "true");
  isPaletteOpen = false;
}

async function runCommandByIndex(index: number): Promise<void> {
  const command = filteredCommands[index];
  if (!command) {
    return;
  }

  closeCommandPalette();
  await command.run();
}

function bindEvents(): void {
  vscodeBtn?.addEventListener("click", () => {
    void openCurrentFileInVSCode();
  });

  commandInputEl?.addEventListener("input", () => {
    selectedCommandIndex = 0;
    renderCommandResults();
  });

  commandInputEl?.addEventListener("keydown", (event) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      if (filteredCommands.length === 0) {
        return;
      }
      selectedCommandIndex = (selectedCommandIndex + 1) % filteredCommands.length;
      renderCommandResults();
      return;
    }

    if (event.key === "ArrowUp") {
      event.preventDefault();
      if (filteredCommands.length === 0) {
        return;
      }
      selectedCommandIndex =
        selectedCommandIndex === 0 ? filteredCommands.length - 1 : selectedCommandIndex - 1;
      renderCommandResults();
      return;
    }

    if (event.key === "Enter") {
      event.preventDefault();
      void runCommandByIndex(selectedCommandIndex);
      return;
    }

    if (event.key === "Escape") {
      event.preventDefault();
      closeCommandPalette();
    }
  });

  commandPaletteEl?.addEventListener("click", (event) => {
    const target = event.target as HTMLElement;
    if (target.closest("[data-close-palette]")) {
      closeCommandPalette();
    }
  });

  window.addEventListener("keydown", (event) => {
    const key = event.key.toLowerCase();
    if ((event.metaKey || event.ctrlKey) && event.shiftKey && key === "p") {
      event.preventDefault();
      openCommandPalette();
      return;
    }

    if (isPaletteOpen && event.key === "Escape") {
      event.preventDefault();
      closeCommandPalette();
    }
  });

  viewerEl?.addEventListener("click", (event) => {
    const target = event.target as HTMLElement;
    const link = target.closest("a[href]") as HTMLAnchorElement | null;
    if (!link) {
      return;
    }

    const href = link.getAttribute("href")?.trim();
    if (!href || href.startsWith("#")) {
      return;
    }

    event.preventDefault();
    void handleLinkClick(href);
  });
}

function buildThemeCommands(): PaletteCommand[] {
  return THEMES.map((theme) => ({
    id: `theme-${theme.id}`,
    label: `Theme: ${theme.label}`,
    keywords: `theme ${theme.keywords}`,
    run: () => {
      applyTheme(theme.id);
      setStatus(`Theme switched to ${currentThemeLabel(theme.id)}.`);
    },
  }));
}

function buildCommands(): PaletteCommand[] {
  return [
    ...buildThemeCommands(),
    {
      id: "reload",
      label: "Reload Document",
      keywords: "reload refresh reread",
      run: async () => {
        await loadDocument("Reloaded from disk.");
      },
    },
    {
      id: "open-vscode",
      label: "Open in VS Code",
      keywords: "open vscode code editor",
      run: async () => {
        await openCurrentFileInVSCode();
      },
    },
  ];
}

window.addEventListener("DOMContentLoaded", async () => {
  restoreTheme();
  commandList = buildCommands();
  bindEvents();

  await listen("basalt://file-changed", async () => {
    await loadDocument("Document updated.");
  });

  await loadDocument("Document loaded.");
});
