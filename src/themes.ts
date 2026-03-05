const THEME_TOKENS = [
  "surface",
  "surface-alt",
  "panel",
  "text",
  "muted",
  "accent",
  "accent-strong",
  "line",
  "code-bg",
  "quote",
  "button-bg",
  "button-hover",
  "status-bg",
] as const;

type ThemeTokenKey = (typeof THEME_TOKENS)[number];

type ThemeTokens = Record<ThemeTokenKey, string>;

type ThemeDefinition = {
  id: string;
  label: string;
  keywords: string;
  tokens: ThemeTokens;
};

export const THEMES = [
  {
    id: "obsidian",
    label: "Obsidian Night",
    keywords: "dark obsidian night",
    tokens: {
      surface: "#111821",
      "surface-alt": "#182330",
      panel: "#101a26",
      text: "#dce7f4",
      muted: "#8ea3ba",
      accent: "#77b9ff",
      "accent-strong": "#5ca5f1",
      line: "#2b3a4b",
      "code-bg": "#0c131d",
      quote: "#2a5f87",
      "button-bg": "#1a2a3b",
      "button-hover": "#22364a",
      "status-bg": "#0f1722",
    },
  },
  {
    id: "paper",
    label: "Graph Paper",
    keywords: "light paper graph",
    tokens: {
      surface: "#f2efe8",
      "surface-alt": "#ece8df",
      panel: "#fffcf5",
      text: "#2a2621",
      muted: "#6e655b",
      accent: "#8c5a2f",
      "accent-strong": "#774a22",
      line: "#d9cfbf",
      "code-bg": "#f6f1e8",
      quote: "#b28747",
      "button-bg": "#ede4d7",
      "button-hover": "#e4d8c9",
      "status-bg": "#e7dfd2",
    },
  },
  {
    id: "grove",
    label: "Moss Grove",
    keywords: "green grove moss",
    tokens: {
      surface: "#0e1e19",
      "surface-alt": "#152a24",
      panel: "#12231e",
      text: "#dbe9e2",
      muted: "#8ea99d",
      accent: "#7fd59f",
      "accent-strong": "#64bd89",
      line: "#29443a",
      "code-bg": "#0a1713",
      quote: "#3e8767",
      "button-bg": "#1b332b",
      "button-hover": "#224136",
      "status-bg": "#0c1714",
    },
  },
  {
    id: "reactor",
    label: "Arc Reactor",
    keywords: "neon blue stark tech futuristic",
    tokens: {
      surface: "#050c17",
      "surface-alt": "#0a1426",
      panel: "#081226",
      text: "#d9f4ff",
      muted: "#83aac7",
      accent: "#2edcff",
      "accent-strong": "#15b8da",
      line: "#1f3a55",
      "code-bg": "#030b14",
      quote: "#1f7fa8",
      "button-bg": "#0f2138",
      "button-hover": "#153050",
      "status-bg": "#07101d",
    },
  },
  {
    id: "foundry",
    label: "Foundry Steel",
    keywords: "industrial graphite steel workshop amber",
    tokens: {
      surface: "#151515",
      "surface-alt": "#1f1f1f",
      panel: "#181818",
      text: "#e7e2d8",
      muted: "#a89e8b",
      accent: "#ffb261",
      "accent-strong": "#e48e33",
      line: "#3a3328",
      "code-bg": "#12100d",
      quote: "#a56e2e",
      "button-bg": "#2a241c",
      "button-hover": "#352d22",
      "status-bg": "#141210",
    },
  },
  {
    id: "hud",
    label: "HUD Crimson",
    keywords: "red cockpit visor tactical command",
    tokens: {
      surface: "#10080b",
      "surface-alt": "#1c0d12",
      panel: "#13090e",
      text: "#ffe2e8",
      muted: "#c89aa8",
      accent: "#ff5f84",
      "accent-strong": "#ff3b68",
      line: "#442030",
      "code-bg": "#0f070a",
      quote: "#ad2d4e",
      "button-bg": "#291019",
      "button-hover": "#341521",
      "status-bg": "#0d0609",
    },
  },
  {
    id: "helios",
    label: "Helios Gold",
    keywords: "light gold titanium luxury bright",
    tokens: {
      surface: "#f5f1e7",
      "surface-alt": "#eee7d9",
      panel: "#fffaf0",
      text: "#2b2417",
      muted: "#72654c",
      accent: "#b07a20",
      "accent-strong": "#8f6017",
      line: "#d9c8a6",
      "code-bg": "#f5eddc",
      quote: "#c08a2f",
      "button-bg": "#eadcbf",
      "button-hover": "#e0cfad",
      "status-bg": "#e7d9bf",
    },
  },
  {
    id: "kanagawa",
    label: "Kanagawa Lotus",
    keywords: "light kanagawa lotus japanese ink paper",
    tokens: {
      surface: "#f2ecbc",
      "surface-alt": "#ebe3b3",
      panel: "#fff7d7",
      text: "#545464",
      muted: "#716e61",
      accent: "#4d699b",
      "accent-strong": "#3f5a8a",
      line: "#d5cea3",
      "code-bg": "#ece4c2",
      quote: "#8e6f3f",
      "button-bg": "#e5ddb2",
      "button-hover": "#dcd3a7",
      "status-bg": "#e2d9ae",
    },
  },
  {
    id: "gruvbox",
    label: "Gruvbox Light",
    keywords: "light gruvbox retro warm terminal",
    tokens: {
      surface: "#fbf1c7",
      "surface-alt": "#f2e5bc",
      panel: "#fff8d5",
      text: "#3c3836",
      muted: "#7c6f64",
      accent: "#b57614",
      "accent-strong": "#9d640f",
      line: "#d5c4a1",
      "code-bg": "#f4e7be",
      quote: "#8f6a2a",
      "button-bg": "#ebdbb2",
      "button-hover": "#e2cf9f",
      "status-bg": "#e6d5ab",
    },
  },
] as const satisfies readonly ThemeDefinition[];

export type ThemeId = (typeof THEMES)[number]["id"];

export const DEFAULT_THEME_ID: ThemeId = "obsidian";

const THEME_BY_ID = new Map<ThemeId, (typeof THEMES)[number]>(THEMES.map((theme) => [theme.id, theme]));
const THEME_ID_SET = new Set<string>(THEMES.map((theme) => theme.id));

export function isThemeId(value: string): value is ThemeId {
  return THEME_ID_SET.has(value);
}

export function currentThemeLabel(themeId: ThemeId): string {
  return THEME_BY_ID.get(themeId)?.label ?? themeId;
}

export function applyThemeVariables(root: HTMLElement, themeId: ThemeId): void {
  const theme = THEME_BY_ID.get(themeId) ?? THEME_BY_ID.get(DEFAULT_THEME_ID);
  if (!theme) {
    return;
  }

  root.dataset.theme = theme.id;
  for (const token of THEME_TOKENS) {
    root.style.setProperty(`--${token}`, theme.tokens[token]);
  }
}
