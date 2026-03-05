import DOMPurify from "dompurify";
import { marked } from "marked";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { applyThemeVariables, currentThemeLabel, DEFAULT_THEME_ID, isThemeId, THEMES, type ThemeId } from "./themes";

type LoadedDocument = {
  path: string;
  fileName: string;
  content: string;
  isMarkdown: boolean;
};

type RecentFileEntry = {
  path: string;
  fileName: string;
  available: boolean;
};

type ResolvedReferenceMap = Record<string, string | null>;

type PaletteCommand = {
  id: string;
  label: string;
  keywords: string;
  run: () => Promise<void> | void;
};

const THEME_STORAGE_KEY = "basalt.theme";
const READER_FONT_SIZE_STORAGE_KEY = "basalt.readerFontSize";
const READER_FONT_SIZE_DEFAULT = 1.03;
const READER_FONT_SIZE_MIN = 0.82;
const READER_FONT_SIZE_MAX = 1.62;
const READER_FONT_SIZE_STEP = 0.08;

const viewerEl = document.querySelector<HTMLElement>("#viewer");
const statusTextEl = document.querySelector<HTMLElement>("#status-text");
const pathEl = document.querySelector<HTMLElement>("#doc-path");
const vscodeBtn = document.querySelector<HTMLButtonElement>("#vscode-btn");

const commandPaletteEl = document.querySelector<HTMLElement>("#command-palette");
const commandInputEl = document.querySelector<HTMLInputElement>("#command-input");
const commandResultsEl = document.querySelector<HTMLUListElement>("#command-results");

const findPanelEl = document.querySelector<HTMLElement>("#find-panel");
const findInputEl = document.querySelector<HTMLInputElement>("#find-input");
const findCountEl = document.querySelector<HTMLElement>("#find-count");
const findPrevBtn = document.querySelector<HTMLButtonElement>("#find-prev");
const findNextBtn = document.querySelector<HTMLButtonElement>("#find-next");
const findCloseBtn = document.querySelector<HTMLButtonElement>("#find-close");

let commandList: PaletteCommand[] = [];
let filteredCommands: PaletteCommand[] = [];
let selectedCommandIndex = 0;
let isPaletteOpen = false;

let isFindOpen = false;
let searchMatches: HTMLElement[] = [];
let activeSearchIndex = -1;

marked.setOptions({ gfm: true, breaks: false });

function setStatus(message: string, isError = false): void {
  if (!statusTextEl) {
    return;
  }
  statusTextEl.textContent = message;
  statusTextEl.dataset.tone = isError ? "error" : "neutral";
}

function applyTheme(theme: ThemeId): void {
  applyThemeVariables(document.documentElement, theme);
  localStorage.setItem(THEME_STORAGE_KEY, theme);
}

function restoreTheme(): void {
  const stored = localStorage.getItem(THEME_STORAGE_KEY);
  const nextTheme: ThemeId = stored && isThemeId(stored) ? stored : DEFAULT_THEME_ID;
  applyTheme(nextTheme);
}

function clampReaderFontSize(value: number): number {
  if (!Number.isFinite(value)) {
    return READER_FONT_SIZE_DEFAULT;
  }
  return Math.min(READER_FONT_SIZE_MAX, Math.max(READER_FONT_SIZE_MIN, value));
}

function applyReaderFontSize(size: number): number {
  const normalized = clampReaderFontSize(size);
  document.documentElement.style.setProperty("--reader-font-size", `${normalized.toFixed(3)}rem`);
  localStorage.setItem(READER_FONT_SIZE_STORAGE_KEY, normalized.toFixed(3));
  return normalized;
}

function restoreReaderFontSize(): void {
  const stored = Number.parseFloat(localStorage.getItem(READER_FONT_SIZE_STORAGE_KEY) ?? "");
  applyReaderFontSize(Number.isNaN(stored) ? READER_FONT_SIZE_DEFAULT : stored);
}

function reportReaderFontSize(size: number): void {
  const percent = Math.round((size / READER_FONT_SIZE_DEFAULT) * 100);
  setStatus(`Reader font size: ${percent}%`);
}

function changeReaderFontSize(delta: number): void {
  const currentRaw = getComputedStyle(document.documentElement).getPropertyValue("--reader-font-size").trim();
  const current = Number.parseFloat(currentRaw);
  const next = applyReaderFontSize((Number.isNaN(current) ? READER_FONT_SIZE_DEFAULT : current) + delta);
  reportReaderFontSize(next);
}

function resetReaderFontSize(): void {
  const next = applyReaderFontSize(READER_FONT_SIZE_DEFAULT);
  reportReaderFontSize(next);
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

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function updateFindCount(): void {
  if (!findCountEl) {
    return;
  }

  if (searchMatches.length === 0) {
    const query = findInputEl?.value.trim() ?? "";
    findCountEl.textContent = query ? "0 matches" : "";
    return;
  }

  const visibleIndex = activeSearchIndex >= 0 ? activeSearchIndex + 1 : 0;
  findCountEl.textContent = `${visibleIndex}/${searchMatches.length}`;
}

function clearSearchHighlights(): void {
  if (!viewerEl) {
    return;
  }

  const marks = viewerEl.querySelectorAll<HTMLElement>("mark[data-search-hit='true']");
  marks.forEach((mark) => {
    const replacement = document.createTextNode(mark.textContent ?? "");
    mark.replaceWith(replacement);
  });

  viewerEl.normalize();
  searchMatches = [];
  activeSearchIndex = -1;
  updateFindCount();
}

function activateSearchMatch(index: number, behavior: ScrollBehavior = "smooth"): void {
  if (searchMatches.length === 0) {
    activeSearchIndex = -1;
    updateFindCount();
    return;
  }

  searchMatches.forEach((match) => match.classList.remove("search-hit-active"));

  const total = searchMatches.length;
  const normalizedIndex = ((index % total) + total) % total;
  activeSearchIndex = normalizedIndex;

  const activeMatch = searchMatches[normalizedIndex];
  activeMatch.classList.add("search-hit-active");
  activeMatch.scrollIntoView({ block: "center", inline: "nearest", behavior });

  updateFindCount();
}

function collectSearchTextNodes(): Text[] {
  if (!viewerEl) {
    return [];
  }

  const walker = document.createTreeWalker(viewerEl, NodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      const value = node.nodeValue;
      if (!value || value.trim().length === 0) {
        return NodeFilter.FILTER_REJECT;
      }

      const parent = node.parentElement;
      if (!parent) {
        return NodeFilter.FILTER_REJECT;
      }

      if (parent.closest("mark[data-search-hit='true']")) {
        return NodeFilter.FILTER_REJECT;
      }

      if (["SCRIPT", "STYLE", "NOSCRIPT"].includes(parent.tagName)) {
        return NodeFilter.FILTER_REJECT;
      }

      return NodeFilter.FILTER_ACCEPT;
    },
  });

  const textNodes: Text[] = [];
  let current = walker.nextNode();

  while (current) {
    if (current.nodeType === Node.TEXT_NODE) {
      textNodes.push(current as Text);
    }
    current = walker.nextNode();
  }

  return textNodes;
}

function runFindSearch(query: string): void {
  clearSearchHighlights();

  const needle = query.trim();
  if (!needle || !viewerEl) {
    updateFindCount();
    return;
  }

  const escapedNeedle = escapeRegExp(needle);
  const textNodes = collectSearchTextNodes();

  textNodes.forEach((node) => {
    const source = node.nodeValue ?? "";
    const matcher = new RegExp(escapedNeedle, "gi");

    if (!matcher.test(source)) {
      return;
    }

    matcher.lastIndex = 0;
    const fragment = document.createDocumentFragment();
    let cursor = 0;
    let result = matcher.exec(source);

    while (result) {
      const [matched] = result;
      const start = result.index;
      const end = start + matched.length;

      if (start > cursor) {
        fragment.append(source.slice(cursor, start));
      }

      const mark = document.createElement("mark");
      mark.dataset.searchHit = "true";
      mark.className = "search-hit";
      mark.textContent = matched;
      fragment.append(mark);
      searchMatches.push(mark);

      cursor = end;
      result = matcher.exec(source);
    }

    if (cursor < source.length) {
      fragment.append(source.slice(cursor));
    }

    node.replaceWith(fragment);
  });

  if (searchMatches.length > 0) {
    activateSearchMatch(0, "auto");
  } else {
    updateFindCount();
  }
}

function setFindPanelVisibility(isOpen: boolean): void {
  if (!findPanelEl) {
    return;
  }

  findPanelEl.hidden = !isOpen;
  findPanelEl.setAttribute("aria-hidden", String(!isOpen));
  isFindOpen = isOpen;
}

function openFindPanel(): void {
  if (!findPanelEl || !findInputEl) {
    return;
  }

  if (isPaletteOpen) {
    closeCommandPalette();
  }

  setFindPanelVisibility(true);

  window.requestAnimationFrame(() => {
    findInputEl.focus();
    findInputEl.select();
  });

  runFindSearch(findInputEl.value);
}

function closeFindPanel(): void {
  setFindPanelVisibility(false);
  if (findInputEl) {
    findInputEl.value = "";
  }
  clearSearchHighlights();
}

function jumpSearchMatch(step: number): void {
  if (searchMatches.length === 0) {
    return;
  }

  activateSearchMatch(activeSearchIndex + step);
}

async function openDocumentDialog(): Promise<void> {
  try {
    const openedPath = await invoke<string | null>("open_document_dialog");
    if (openedPath) {
      setStatus(`Opened ${openedPath}.`);
    } else {
      setStatus("Open canceled.");
    }
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    setStatus(`Unable to open file: ${message}`, true);
  }
}

async function openDocumentPath(path: string): Promise<void> {
  try {
    await invoke("open_document_path", { path });
    setStatus(`Opened ${path}.`);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    setStatus(`Unable to open file: ${message}`, true);
  }
}

function renderRecentFilesMarkup(entries: RecentFileEntry[]): string {
  if (entries.length === 0) {
    return "";
  }

  const items = entries
    .map((entry) => {
      const label = `${escapeHtml(entry.fileName)}<span>${escapeHtml(entry.path)}</span>`;

      if (!entry.available) {
        return `<li><button class="recent-btn" type="button" disabled>${label} <em>(missing)</em></button></li>`;
      }

      return `<li><button class="recent-btn" type="button" data-open-recent="${escapeHtml(entry.path)}">${label}</button></li>`;
    })
    .join("");

  return `
    <section class="recent-state">
      <h2>Recent files</h2>
      <ul class="recent-list">${items}</ul>
    </section>
  `;
}

async function renderEmptyState(message?: string): Promise<void> {
  if (!viewerEl) {
    return;
  }

  let recents: RecentFileEntry[] = [];
  try {
    recents = await invoke<RecentFileEntry[]>("list_recent_files");
  } catch {
    recents = [];
  }

  const details = message
    ? `<p class="empty-error">${escapeHtml(message)}</p>`
    : "<p>No file is currently attached to this window.</p>";

  const recentsMarkup = renderRecentFilesMarkup(recents);

  viewerEl.innerHTML = `
    <section class="empty-state">
      <h1>Basalt is waiting for a document</h1>
      ${details}
      <p>Open a Markdown file with <kbd>Cmd/Ctrl</kbd>+<kbd>O</kbd> or pick one below.</p>
      <button class="empty-open-btn" type="button" data-open-dialog>Open Markdown File...</button>
      ${recentsMarkup}
      <pre><code>basalt ./notes/today.md ./reports ./config.json</code></pre>
      <pre><code>basalt watch ./reports</code></pre>
    </section>
  `;

  if (pathEl) {
    pathEl.textContent = "No file attached to this window.";
    pathEl.title = "No file attached to this window.";
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

function asFencedCodeBlock(content: string): string {
  const runs = content.match(/`+/g) ?? [];
  const longest = runs.reduce((max, run) => Math.max(max, run.length), 0);
  const fence = "`".repeat(Math.max(3, longest + 1));
  return `${fence}\n${content}\n${fence}`;
}

async function renderDocument(document: LoadedDocument): Promise<void> {
  if (!viewerEl) {
    return;
  }

  clearSearchHighlights();

  if (pathEl) {
    pathEl.textContent = document.path;
    pathEl.title = document.path;
  }

  const source = document.isMarkdown ? document.content : asFencedCodeBlock(document.content);
  const rendered = marked.parse(source, { async: false }) as string;
  const sanitized = DOMPurify.sanitize(rendered);
  viewerEl.innerHTML = sanitized;

  if (document.isMarkdown) {
    await hydrateReferences();
  }

  if (isFindOpen && findInputEl?.value.trim()) {
    runFindSearch(findInputEl.value);
  }
}

async function loadDocument(reason: string): Promise<void> {
  try {
    const document = await invoke<LoadedDocument>("load_document");
    await renderDocument(document);
    setStatus(reason);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    await renderEmptyState(message);
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
  setFindPanelVisibility(false);

  vscodeBtn?.addEventListener("click", () => {
    void openCurrentFileInVSCode();
  });

  findInputEl?.addEventListener("input", () => {
    runFindSearch(findInputEl.value);
  });

  findInputEl?.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      jumpSearchMatch(event.shiftKey ? -1 : 1);
      return;
    }

    if (event.key === "Escape") {
      event.preventDefault();
      closeFindPanel();
    }
  });

  findPrevBtn?.addEventListener("click", () => {
    jumpSearchMatch(-1);
  });

  findNextBtn?.addEventListener("click", () => {
    jumpSearchMatch(1);
  });

  findCloseBtn?.addEventListener("click", () => {
    closeFindPanel();
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
    const hasCommandModifier = event.metaKey || event.ctrlKey;
    const plusPressed = key === "+" || key === "=" || event.code === "NumpadAdd";
    const minusPressed = key === "-" || key === "_" || event.code === "NumpadSubtract";

    if (hasCommandModifier && !event.altKey && plusPressed) {
      event.preventDefault();
      changeReaderFontSize(READER_FONT_SIZE_STEP);
      return;
    }

    if (hasCommandModifier && !event.altKey && minusPressed) {
      event.preventDefault();
      changeReaderFontSize(-READER_FONT_SIZE_STEP);
      return;
    }

    if (hasCommandModifier && !event.altKey && key === "0") {
      event.preventDefault();
      resetReaderFontSize();
      return;
    }

    if (hasCommandModifier && !event.shiftKey && key === "f") {
      event.preventDefault();
      openFindPanel();
      return;
    }

    if (hasCommandModifier && !event.shiftKey && key === "o") {
      event.preventDefault();
      void openDocumentDialog();
      return;
    }

    if (hasCommandModifier && event.shiftKey && key === "p") {
      event.preventDefault();
      openCommandPalette();
      return;
    }

    if (isFindOpen && event.key === "Escape") {
      event.preventDefault();
      closeFindPanel();
      return;
    }

    if (isPaletteOpen && event.key === "Escape") {
      event.preventDefault();
      closeCommandPalette();
    }
  });

  viewerEl?.addEventListener("click", (event) => {
    const target = event.target as HTMLElement;

    const openDialogBtn = target.closest<HTMLElement>("[data-open-dialog]");
    if (openDialogBtn) {
      event.preventDefault();
      void openDocumentDialog();
      return;
    }

    const recentBtn = target.closest<HTMLButtonElement>("[data-open-recent]");
    const recentPath = recentBtn?.dataset.openRecent;
    if (recentPath) {
      event.preventDefault();
      void openDocumentPath(recentPath);
      return;
    }

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
      id: "open-file",
      label: "Open File...",
      keywords: "open file dialog cmd+o",
      run: async () => {
        await openDocumentDialog();
      },
    },
    {
      id: "find",
      label: "Find in Document",
      keywords: "search find cmd+f",
      run: () => {
        openFindPanel();
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
  restoreReaderFontSize();
  commandList = buildCommands();
  bindEvents();

  await listen("basalt://file-changed", async () => {
    await loadDocument("Document updated.");
  });

  await listen("basalt://focus-search", () => {
    openFindPanel();
  });

  await loadDocument("Document loaded.");
});
