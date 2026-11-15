import { describe, expect, it } from "vitest";
import {
  compareTauriVersions,
  formatVersionTable,
  parseCargoLockVersions,
  parseMajorMinor,
  parsePackageLockVersions,
} from "./check-tauri-versions.mjs";

const CARGO_FIXTURE_MATCH = `
version = 4

[[package]]
name = "tauri"
version = "2.12.3"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "tauri-plugin-dialog"
version = "2.8.4"

[[package]]
name = "tauri-plugin-single-instance"
version = "2.5.5"
`;

const NPM_FIXTURE_MATCH = JSON.stringify({
  lockfileVersion: 3,
  packages: {
    "": { version: "1.1.0" },
    "node_modules/@tauri-apps/api": { version: "2.12.4" },
    "node_modules/@tauri-apps/cli": { version: "2.12.4" },
    "node_modules/@tauri-apps/plugin-dialog": { version: "2.8.4" },
  },
});

describe("check-tauri-versions", () => {
  describe("parseMajorMinor", () => {
    it("extracts major.minor from standard and prerelease versions", () => {
      expect(parseMajorMinor("2.12.4")).toBe("2.12");
      expect(parseMajorMinor("2.8.0-alpha.1")).toBe("2.8");
      expect(parseMajorMinor("v2.9.0")).toBe("2.9");
      expect(parseMajorMinor(null)).toBeNull();
      expect(parseMajorMinor("invalid")).toBeNull();
    });
  });

  describe("parseCargoLockVersions", () => {
    it("parses package names and versions from Cargo.lock blocks", () => {
      const map = parseCargoLockVersions(CARGO_FIXTURE_MATCH);
      expect(map.get("tauri")).toEqual(["2.12.3"]);
      expect(map.get("tauri-plugin-dialog")).toEqual(["2.8.4"]);
      expect(map.get("tauri-plugin-single-instance")).toEqual(["2.5.5"]);
    });
  });

  describe("parsePackageLockVersions", () => {
    it("parses top-level node_modules versions and ignores nested ones", () => {
      const lock = JSON.stringify({
        lockfileVersion: 3,
        packages: {
          "node_modules/@tauri-apps/api": { version: "2.12.4" },
          "node_modules/some-pkg/node_modules/@tauri-apps/api": { version: "2.0.0" },
        },
      });
      const map = parsePackageLockVersions(lock);
      expect(map.get("@tauri-apps/api")).toBe("2.12.4");
      expect(map.size).toBe(1);
    });
  });

  describe("compareTauriVersions", () => {
    it("passes when major.minor matches even if patch versions differ and tauri-cli is absent", () => {
      const result = compareTauriVersions(CARGO_FIXTURE_MATCH, NPM_FIXTURE_MATCH);
      expect(result.ok).toBe(true);
      expect(result.pairs).toHaveLength(2);
      expect(result.pairs.every((p) => p.status === "OK")).toBe(true);
    });

    it("fails when a plugin pair differs in minor version", () => {
      const cargoMismatch = CARGO_FIXTURE_MATCH.replace('version = "2.8.4"', 'version = "2.7.2"');
      const result = compareTauriVersions(cargoMismatch, NPM_FIXTURE_MATCH);
      expect(result.ok).toBe(false);
      const dialog = result.pairs.find((p) => p.cargoName === "tauri-plugin-dialog");
      expect(dialog?.match).toBe(false);
      expect(dialog?.status).toBe("MISMATCH");
      expect(dialog?.cargoMajorMinor).toBe("2.7");
      expect(dialog?.npmMajorMinor).toBe("2.8");
    });

    it("checks tauri-cli <-> @tauri-apps/cli when both exist", () => {
      const cargoWithCli = `${CARGO_FIXTURE_MATCH}\n[[package]]\nname = "tauri-cli"\nversion = "2.11.0"\n`;
      const result = compareTauriVersions(cargoWithCli, NPM_FIXTURE_MATCH);
      expect(result.ok).toBe(false);
      const cli = result.pairs.find((p) => p.cargoName === "tauri-cli");
      expect(cli?.status).toBe("MISMATCH");
    });

    it("fails if an npm plugin has no matching Cargo.lock crate", () => {
      const npmWithMissing = JSON.stringify({
        lockfileVersion: 3,
        packages: {
          "node_modules/@tauri-apps/api": { version: "2.12.4" },
          "node_modules/@tauri-apps/plugin-http": { version: "2.9.0" },
        },
      });
      const result = compareTauriVersions(CARGO_FIXTURE_MATCH, npmWithMissing);
      expect(result.ok).toBe(false);
      const http = result.pairs.find((p) => p.cargoName === "tauri-plugin-http");
      expect(http?.status).toBe("MISSING");
    });
  });

  describe("formatVersionTable", () => {
    it("renders a readable table with header and status rows", () => {
      const { pairs } = compareTauriVersions(CARGO_FIXTURE_MATCH, NPM_FIXTURE_MATCH);
      const table = formatVersionTable(pairs);
      expect(table).toContain("tauri");
      expect(table).toContain("@tauri-apps/api");
      expect(table).toContain("2.12 / 2.12");
      expect(table).toContain("OK");
    });
  });
});
