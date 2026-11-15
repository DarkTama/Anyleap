import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

/**
 * Extracts `major.minor` from a semver string (e.g. "2.12.4-alpha.1" -> "2.12").
 * @param {string | null | undefined} version
 * @returns {string | null}
 */
export function parseMajorMinor(version) {
  if (typeof version !== "string") return null;
  const match = version.trim().match(/^v?(\d+)\.(\d+)/);
  return match ? `${match[1]}.${match[2]}` : null;
}

/**
 * Parses package names and resolved versions from a Cargo.lock string.
 * @param {string} cargoLockText
 * @returns {Map<string, string[]>}
 */
export function parseCargoLockVersions(cargoLockText) {
  const packages = new Map();
  let inPackage = false;
  let currentName = null;
  let currentVersion = null;

  const flush = () => {
    if (inPackage && currentName && currentVersion) {
      const existing = packages.get(currentName);
      if (existing) {
        if (!existing.includes(currentVersion)) {
          existing.push(currentVersion);
        }
      } else {
        packages.set(currentName, [currentVersion]);
      }
    }
    currentName = null;
    currentVersion = null;
  };

  for (const rawLine of cargoLockText.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (line === "[[package]]") {
      flush();
      inPackage = true;
      continue;
    }
    if (line.startsWith("[") && line.endsWith("]")) {
      flush();
      inPackage = false;
      continue;
    }
    if (!inPackage) continue;

    const nameMatch = line.match(/^name\s*=\s*"([^"]+)"/);
    if (nameMatch) {
      currentName = nameMatch[1];
      continue;
    }
    const versionMatch = line.match(/^version\s*=\s*"([^"]+)"/);
    if (versionMatch) {
      currentVersion = versionMatch[1];
    }
  }
  flush();

  return packages;
}

/**
 * Parses top-level package versions from a package-lock.json string.
 * @param {string} packageLockText
 * @returns {Map<string, string>}
 */
export function parsePackageLockVersions(packageLockText) {
  const data = JSON.parse(packageLockText);
  const versions = new Map();

  if (data && typeof data.packages === "object" && data.packages !== null) {
    for (const [pkgPath, entry] of Object.entries(data.packages)) {
      if (!pkgPath.startsWith("node_modules/")) continue;
      const pkgName = pkgPath.slice("node_modules/".length);
      if (pkgName.includes("node_modules/")) continue;
      if (entry && typeof entry.version === "string") {
        versions.set(pkgName, entry.version);
      }
    }
    return versions;
  }

  if (data && typeof data.dependencies === "object" && data.dependencies !== null) {
    for (const [pkgName, entry] of Object.entries(data.dependencies)) {
      if (entry && typeof entry.version === "string") {
        versions.set(pkgName, entry.version);
      }
    }
  }

  return versions;
}

/**
 * Discovers and compares Tauri Rust/JS package pairs on major.minor.
 * @param {string} cargoLockText
 * @param {string} packageLockText
 * @returns {{
 *   ok: boolean,
 *   pairs: Array<{
 *     cargoName: string,
 *     npmName: string,
 *     cargoVersions: string[],
 *     npmVersion: string | null,
 *     cargoMajorMinor: string | null,
 *     npmMajorMinor: string | null,
 *     match: boolean,
 *     status: string,
 *   }>
 * }}
 */
export function compareTauriVersions(cargoLockText, packageLockText) {
  const cargoMap = parseCargoLockVersions(cargoLockText);
  const npmMap = parsePackageLockVersions(packageLockText);

  /** @type {Array<[string, string]>} */
  const candidatePairs = [];

  if (cargoMap.has("tauri") || npmMap.has("@tauri-apps/api")) {
    candidatePairs.push(["tauri", "@tauri-apps/api"]);
  }

  if (cargoMap.has("tauri-cli") && npmMap.has("@tauri-apps/cli")) {
    candidatePairs.push(["tauri-cli", "@tauri-apps/cli"]);
  }

  const pluginSuffixes = new Set();
  for (const npmName of npmMap.keys()) {
    if (npmName.startsWith("@tauri-apps/plugin-")) {
      pluginSuffixes.add(npmName.slice("@tauri-apps/plugin-".length));
    }
  }
  for (const suffix of [...pluginSuffixes].sort()) {
    candidatePairs.push([`tauri-plugin-${suffix}`, `@tauri-apps/plugin-${suffix}`]);
  }

  const pairs = candidatePairs.map(([cargoName, npmName]) => {
    const cargoVersions = cargoMap.get(cargoName) ?? [];
    const npmVersion = npmMap.get(npmName) ?? null;

    const cargoMajorMinors = cargoVersions.map(parseMajorMinor);
    const npmMajorMinor = parseMajorMinor(npmVersion);

    let match = false;
    let status = "OK";

    if (cargoVersions.length === 0 || npmVersion === null) {
      status = "MISSING";
    } else if (cargoMajorMinors.some((mm) => mm === null) || npmMajorMinor === null) {
      status = "INVALID";
    } else if (cargoMajorMinors.every((mm) => mm === npmMajorMinor)) {
      match = true;
      status = "OK";
    } else {
      status = "MISMATCH";
    }

    const uniqueCargoMm = [...new Set(cargoMajorMinors.filter(Boolean))];
    const cargoMajorMinor = uniqueCargoMm.length === 1 ? uniqueCargoMm[0] : uniqueCargoMm.join(", ") || null;

    return {
      cargoName,
      npmName,
      cargoVersions,
      npmVersion,
      cargoMajorMinor,
      npmMajorMinor,
      match,
      status,
    };
  });

  const ok = pairs.length > 0 && pairs.every((p) => p.match);
  return { ok, pairs };
}

/**
 * Formats comparison results as an aligned ASCII table.
 * @param {ReturnType<typeof compareTauriVersions>["pairs"]} pairs
 * @returns {string}
 */
export function formatVersionTable(pairs) {
  const headers = ["Crate", "NPM Package", "Cargo.lock", "package-lock.json", "Major.Minor", "Status"];
  const rows = pairs.map((p) => [
    p.cargoName,
    p.npmName,
    p.cargoVersions.length > 0 ? p.cargoVersions.join(", ") : "<missing>",
    p.npmVersion ?? "<missing>",
    `${p.cargoMajorMinor ?? "?"} / ${p.npmMajorMinor ?? "?"}`,
    p.status,
  ]);

  const allRows = [headers, ...rows];
  const widths = headers.map((_, col) =>
    Math.max(...allRows.map((row) => row[col].length)),
  );

  const formatRow = (row) =>
    row.map((cell, i) => cell.padEnd(widths[i])).join(" | ");
  const separator = widths.map((w) => "-".repeat(w)).join("-+-");

  return [formatRow(headers), separator, ...rows.map(formatRow)].join("\n");
}

function main() {
  const __dirname = path.dirname(fileURLToPath(import.meta.url));
  const root = path.resolve(__dirname, "..");
  const cargoLockPath = path.join(root, "src-tauri", "Cargo.lock");
  const packageLockPath = path.join(root, "package-lock.json");

  const cargoLockText = fs.readFileSync(cargoLockPath, "utf8");
  const packageLockText = fs.readFileSync(packageLockPath, "utf8");

  const result = compareTauriVersions(cargoLockText, packageLockText);
  console.log(formatVersionTable(result.pairs));

  if (!result.ok) {
    console.error("\nTauri Rust/NPM major.minor version mismatch detected.");
    process.exit(1);
  }
}

const isEntrypoint =
  process.argv[1] &&
  pathToFileURL(fs.realpathSync(process.argv[1])).href === import.meta.url;

if (isEntrypoint) {
  main();
}
