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

const THEME_STORAGE_KEY = "basalt.theme";

const viewerEl = document.querySelector<HTMLElement>("#viewer");
const statusEl = document.querySelector<HTMLElement>("#status");
const nameEl = document.querySelector<HTMLElement>("#doc-name");
const pathEl = document.querySelector<HTMLElement>("#doc-path");
const themeSelectEl = document.querySelector<HTMLSelectElement>("#theme-select");
const reloadBtn = document.querySelector<HTMLButtonElement>("#reload-btn");
const vscodeBtn = document.querySelector<HTMLButtonElement>("#vscode-btn");

marked.setOptions({ gfm: true, breaks: false });

function setStatus(message: string, isError = false): void {
  if (!statusEl) {
    return;
  }
  statusEl.textContent = message;
  statusEl.dataset.tone = isError ? "error" : "neutral";
}

function applyTheme(theme: string): void {
  document.documentElement.dataset.theme = theme;
  localStorage.setItem(THEME_STORAGE_KEY, theme);
}

function restoreTheme(): void {
  if (!themeSelectEl) {
    return;
  }

  const stored = localStorage.getItem(THEME_STORAGE_KEY) ?? "obsidian";
  const available = new Set(Array.from(themeSelectEl.options).map((option) => option.value));
  const nextTheme = available.has(stored) ? stored : "obsidian";

  themeSelectEl.value = nextTheme;
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

  if (nameEl) {
    nameEl.textContent = "No document loaded";
  }
  if (pathEl) {
    pathEl.textContent = "Open via terminal to get started.";
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

  if (nameEl) {
    nameEl.textContent = document.fileName;
  }
  if (pathEl) {
    pathEl.textContent = document.path;
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

function bindEvents(): void {
  themeSelectEl?.addEventListener("change", (event) => {
    const target = event.target as HTMLSelectElement;
    applyTheme(target.value);
    setStatus(`Theme switched to ${target.options[target.selectedIndex]?.text ?? target.value}.`);
  });

  reloadBtn?.addEventListener("click", () => {
    void loadDocument("Reloaded from disk.");
  });

  vscodeBtn?.addEventListener("click", async () => {
    try {
      await invoke("open_in_vscode");
      setStatus("Opened current file in VS Code.");
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setStatus(`Unable to open VS Code: ${message}`, true);
    }
  });

  viewerEl?.addEventListener("click", (event) => {
    const target = event.target as HTMLElement;
    const link = target.closest("a[href]") as HTMLAnchorElement | null;
    if (!link) {
      return;
    }

    const href = link.getAttribute("href")?.trim();
    if (!href) {
      return;
    }

    if (href.startsWith("#")) {
      return;
    }

    event.preventDefault();
    void handleLinkClick(href);
  });
}

window.addEventListener("DOMContentLoaded", async () => {
  restoreTheme();
  bindEvents();

  await listen("basalt://file-changed", async () => {
    await loadDocument("Document updated.");
  });

  await loadDocument("Document loaded.");
});
