import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

const SRC_DIR = path.resolve(import.meta.dirname, "..");
const INDEX_CSS_PATH = path.join(SRC_DIR, "index.css");

function parseRootTokens(css: string): Record<string, string> {
  const rootMatch = css.match(/:root\s*\{([\s\S]*?)\}/);
  if (!rootMatch) return {};
  const tokens: Record<string, string> = {};
  const declRegex = /(--[a-z0-9-]+)\s*:\s*([^;]+);/gi;
  for (const match of rootMatch[1].matchAll(declRegex)) {
    tokens[match[1]] = match[2].trim();
  }
  return tokens;
}

function hexToRgb(hex: string): [number, number, number] {
  const clean = hex.replace(/^#/, "");
  if (clean.length !== 6) {
    throw new Error(`Expected 6-digit hex color, got "${hex}"`);
  }
  const n = Number.parseInt(clean, 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function relativeLuminance(hex: string): number {
  const channels = hexToRgb(hex).map((v) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2];
}

function contrastRatio(fgHex: string, bgHex: string): number {
  const l1 = relativeLuminance(fgHex);
  const l2 = relativeLuminance(bgHex);
  const lighter = Math.max(l1, l2);
  const darker = Math.min(l1, l2);
  return (lighter + 0.05) / (darker + 0.05);
}

function walkSourceFiles(dir: string): string[] {
  const results: string[] = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      results.push(...walkSourceFiles(full));
    } else if (
      /\.(ts|tsx|css)$/.test(entry.name) &&
      !/\.test\.(ts|tsx)$/.test(entry.name)
    ) {
      results.push(full);
    }
  }
  return results;
}

describe("SPEC-26 Ink & Gold theme tokens & CSS hygiene", () => {
  const css = fs.readFileSync(INDEX_CSS_PATH, "utf8");
  const tokens = parseRootTokens(css);

  it("defines all required CSS custom properties and exposes them to Tailwind @theme", () => {
    const expectedHexes: Record<string, string> = {
      "--ink-950": "#12141F",
      "--ink-900": "#1B1E2E",
      "--ink-800": "#2A2D40",
      "--ink-700": "#3A3D52",
      "--text-strong": "#ECEAE3",
      "--text": "#C9CBD6",
      "--text-muted": "#8B8FA3",
      "--gold-400": "#D8B46A",
      "--gold-600": "#A9822F",
      "--mist-300": "#B7C9D6",
      "--status-success": "#6DB88B",
      "--status-success-bg": "#1B2C28",
      "--status-warning": "#E0A858",
      "--status-warning-bg": "#2A231D",
      "--status-danger": "#E48282",
      "--status-danger-bg": "#2B1A22",
      "--status-danger-solid": "#B83D3D",
    };

    for (const [name, hex] of Object.entries(expectedHexes)) {
      expect(tokens[name]?.toUpperCase()).toBe(hex.toUpperCase());
      expect(css).toContain(`--color-${name.slice(2)}: var(${name})`);
    }

    expect(tokens["--ease-out"]).toBe("cubic-bezier(0.23, 1, 0.32, 1)");
    expect(css).toMatch(/@media\s*\(hover:\s*hover\)\s*and\s*\(pointer:\s*fine\)/);
    expect(css).toMatch(/@media\s*\(prefers-reduced-motion:\s*reduce\)/);
  });

  it("meets WCAG >= 4.5:1 contrast for key text, accent, and status pairs", () => {
    const pairs: Array<[string, string]> = [
      ["--text-strong", "--ink-950"],
      ["--text-strong", "--ink-900"],
      ["--text-strong", "--ink-800"],
      ["--text", "--ink-950"],
      ["--text", "--ink-900"],
      ["--text", "--ink-800"],
      ["--text-muted", "--ink-950"],
      ["--text-muted", "--ink-900"],
      ["--gold-400", "--ink-950"],
      ["--gold-400", "--ink-900"],
      ["--gold-400", "--ink-800"],
      ["--ink-950", "--gold-400"],
      ["--ink-950", "--gold-600"],
      ["--mist-300", "--ink-950"],
      ["--mist-300", "--ink-900"],
      ["--mist-300", "--ink-800"],
      ["--status-success", "--status-success-bg"],
      ["--status-success", "--ink-950"],
      ["--status-warning", "--status-warning-bg"],
      ["--status-warning", "--ink-950"],
      ["--status-danger", "--status-danger-bg"],
      ["--status-danger", "--ink-950"],
      ["--text-strong", "--status-danger-solid"],
    ];

    for (const [fg, bg] of pairs) {
      const ratio = contrastRatio(tokens[fg], tokens[bg]);
      expect(
        ratio,
        `Contrast ratio for ${fg} (${tokens[fg]}) on ${bg} (${tokens[bg]}) was ${ratio.toFixed(2)}`
      ).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("contains no gradients, backdrop-blur, drop-shadow, coloured shadows, transition-all, or decorative pulse/ping/bounce under src/", () => {
    const forbiddenChecks: Array<{ label: string; regex: RegExp }> = [
      { label: "gradient class or CSS gradient", regex: new RegExp(["bg-", "gradient|linear-", "gradient|radial-", "gradient|<linear", "Gradient|<radial", "Gradient"].join(""), "i") },
      { label: "backdrop blur", regex: new RegExp(["back", "drop-blur"].join("")) },
      { label: "drop shadow", regex: new RegExp(["drop-", "shadow"].join("")) },
      { label: "arbitrary shadow glow", regex: new RegExp(["shadow-", "\\["].join("")) },
      { label: "transition all", regex: new RegExp(["transition-", "all|transition:\\s*all"].join("")) },
      { label: "decorative loop animation", regex: new RegExp(["animate-", "pulse|animate-", "ping|animate-", "bounce"].join("")) },
    ];

    const files = walkSourceFiles(SRC_DIR);
    const violations: string[] = [];

    for (const file of files) {
      const content = fs.readFileSync(file, "utf8");
      const lines = content.split(/\r?\n/);
      lines.forEach((line, idx) => {
        for (const check of forbiddenChecks) {
          if (check.regex.test(line)) {
            violations.push(
              `${path.relative(SRC_DIR, file)}:${idx + 1} [${check.label}] ${line.trim()}`
            );
          }
        }
      });
    }

    expect(violations).toEqual([]);
  });
});
