import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";

// AGENTS.md section 1.3: no third-party product names in code, comments, tests or specs.
// The denylist is stored reversed so this file does not name any of them itself.
const DENYLIST = ["yppcrse", "xed", "gnusmas"].map((w) => w.split("").reverse().join(""));
const PATTERN = new RegExp(`\\b(${DENYLIST.join("|")})\\b`, "i");

const root = process.cwd();
const SELF = join("src", "lib", "noThirdPartyNames.test.ts");

function walk(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    if (statSync(full).isDirectory()) walk(full, out);
    else if (/\.(ts|tsx|css|md)$/.test(name)) out.push(full);
  }
  return out;
}

describe("third-party product names", () => {
  it("do not appear in src/** or the SPEC-28 documents", () => {
    const files = [
      ...walk(join(root, "src")),
      ...readdirSync(join(root, "docs", "specs"))
        .filter((n) => n.startsWith("SPEC-28"))
        .map((n) => join(root, "docs", "specs", n)),
      ...readdirSync(join(root, "docs", "adr"))
        .filter((n) => n.startsWith("0005"))
        .map((n) => join(root, "docs", "adr", n)),
    ].filter((f) => relative(root, f) !== SELF);
    const hits = files.filter((f) => PATTERN.test(readFileSync(f, "utf8"))).map((f) => relative(root, f));
    expect(hits).toEqual([]);
  });
});
