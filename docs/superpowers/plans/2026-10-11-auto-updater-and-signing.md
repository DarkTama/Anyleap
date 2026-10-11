# Auto Updater, Minisign Signing & Commit Changelog Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement seamless in-app auto-updating with Minisign Ed25519 signatures, background download progress, silent install, instant app relaunch, rich commit changelog display, and automated GitHub Actions release synchronization.

**Architecture:** Frontend uses Zustand store (`appUpdateStore`) and GitHub Release service (`updateChecker`) modeled after OpenTerm to display markdown changelogs and download progress. Backend initializes `tauri-plugin-updater` and `tauri-plugin-process` with `updaterJsonPreferNsis` for silent background Windows NSIS updates. GitHub Actions workflow automates tag version syncing and commit log generation.

**Tech Stack:** Tauri 2 (Rust) · React 19 + TypeScript + Vite · Tailwind v4 · Zustand · `@tauri-apps/plugin-updater` · `@tauri-apps/plugin-process` · Vitest

**Spec:** `docs/specs/SPEC-16-in-app-updater-and-signing.md`

## Global Constraints

- Never block application startup if update check fails (silent error handling and 5s network timeouts).
- Cache GitHub API responses for 30 minutes in `localStorage` to avoid rate limiting.
- Silent background NSIS execution on Windows without manual wizard prompts.
- Maintain strict backward compatibility for offline use.
- All frontend tests must run cleanly via `npm test` (Vitest).

---

## File Structure

```
.github/workflows/
└── release.yml                             # Modified: tag sync, commit changelog, tauri updater flags

src-tauri/
├── Cargo.toml                              # Modified: add tauri-plugin-updater & tauri-plugin-process
├── tauri.conf.json                         # Modified: plugins.updater, bundle.createUpdaterArtifacts
└── src/main.rs                             # Modified: register updater and process plugins

src/
├── services/
│   ├── updateChecker.ts                    # Created: GitHub API fetcher, semver parsing, changelog cache
│   └── __tests__/
│       └── updateChecker.test.ts           # Created: Vitest unit tests for updateChecker
├── stores/
│   ├── appUpdateStore.ts                   # Created: Zustand store for install phases & progress
│   └── __tests__/
│       └── appUpdateStore.test.ts          # Created: Vitest unit tests for appUpdateStore
├── components/
│   └── shell/
│       ├── UpdateBadge.tsx                 # Created: Top bar update availability chip
│       └── UpdateModal.tsx                 # Created: Markdown changelog viewer & install/restart modal
├── lib/
│   └── updateCheck.ts                      # Modified/Deprecated: forward legacy calls to updateChecker
└── App.tsx                                 # Modified: trigger background update probe on mount
```

---

### Task 1: Rust & Tauri Configuration (Plugins & Manifests)

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/src/main.rs`
- Modify: `package.json`

**Interfaces:**
- Consumes: `@tauri-apps/plugin-updater` ^2.0.0, `@tauri-apps/plugin-process` ^2.0.0
- Produces: Tauri runtime configured with updater plugin and silent NSIS artifact generation

- [x] **Step 1: Install frontend npm dependencies**

Run: `npm install @tauri-apps/plugin-updater@~2.0.0 @tauri-apps/plugin-process@~2.0.0`

- [x] **Step 2: Add Rust crates to `src-tauri/Cargo.toml`**

Add under `[dependencies]`:
```toml
tauri-plugin-updater = "2"
tauri-plugin-process = "2"
```

- [x] **Step 3: Update `src-tauri/tauri.conf.json`**

In `src-tauri/tauri.conf.json`:
1. Under `"bundle"`: add `"createUpdaterArtifacts": true`.
2. Under `"plugins"`: add:
```json
"updater": {
  "pubkey": "REPLACE_WITH_UPDATER_PUBLIC_KEY",
  "endpoints": [
    "https://github.com/DarkTama/Anyleap/releases/latest/download/latest.json"
  ]
}
```

- [x] **Step 4: Register plugins in `src-tauri/src/main.rs`**

Add `.plugin(tauri_plugin_updater::Builder::new().build())` and `.plugin(tauri_plugin_process::init())` inside `tauri::Builder::default()`.

- [x] **Step 5: Verify Cargo check**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: SUCCESS (0 errors).

- [x] **Step 6: Commit Task 1**

Run: `git commit -am "feat(updater): configure tauri updater and process plugins"`

---

### Task 2: Release Checker Service (`src/services/updateChecker.ts`)

**Files:**
- Create: `src/services/updateChecker.ts`
- Create: `src/services/__tests__/updateChecker.test.ts`

**Interfaces:**
- Consumes: GitHub Releases REST API (`https://api.github.com/repos/DarkTama/Anyleap/releases`)
- Produces:
  - `checkLatestRelease(force?: boolean, currentVersion?: string): Promise<UpdateInfo | null>`
  - `fetchReleasesList(force?: boolean): Promise<ReleaseItem[]>`
  - `calculateVersionDistance(releases: ReleaseItem[], currentVersion: string): VersionDistanceResult`
  - `isNewerVersion(remote: string, current: string): boolean`

- [x] **Step 1: Write failing tests in `src/services/__tests__/updateChecker.test.ts`**

```typescript
import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  parseSemver,
  isNewerVersion,
  calculateVersionDistance,
  checkLatestRelease,
  type ReleaseItem,
} from "../updateChecker";

describe("updateChecker", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.restoreAllMocks();
  });

  it("parses valid semver versions", () => {
    expect(parseSemver("0.9.6")).toEqual([0, 9, 6]);
    expect(parseSemver("v1.2.3")).toEqual([1, 2, 3]);
    expect(parseSemver("invalid")).toBeNull();
  });

  it("correctly identifies newer versions", () => {
    expect(isNewerVersion("0.9.7", "0.9.6")).toBe(true);
    expect(isNewerVersion("1.0.0", "0.9.6")).toBe(true);
    expect(isNewerVersion("0.9.6", "0.9.6")).toBe(false);
    expect(isNewerVersion("0.9.5", "0.9.6")).toBe(false);
  });

  it("calculates version distance and uninstalled releases", () => {
    const releases: ReleaseItem[] = [
      { id: 2, tagName: "v0.9.8", name: "v0.9.8", publishedAt: "", body: "Fix B", htmlUrl: "", isPrerelease: false },
      { id: 1, tagName: "v0.9.7", name: "v0.9.7", publishedAt: "", body: "Fix A", htmlUrl: "", isPrerelease: false },
      { id: 0, tagName: "v0.9.6", name: "v0.9.6", publishedAt: "", body: "Initial", htmlUrl: "", isPrerelease: false },
    ];
    const dist = calculateVersionDistance(releases, "0.9.6");
    expect(dist.isLatest).toBe(false);
    expect(dist.behindCount).toBe(2);
    expect(dist.latestVersion).toBe("0.9.8");
    expect(dist.uninstalledReleases).toHaveLength(2);
  });
});
```

- [x] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/services/__tests__/updateChecker.test.ts`
Expected: FAIL ("Cannot find module '../updateChecker'")

- [x] **Step 3: Implement `src/services/updateChecker.ts`**

Implement `parseSemver`, `isNewerVersion`, `checkLatestRelease`, `fetchReleasesList`, `calculateVersionDistance` with 30-minute `localStorage` cache.

- [x] **Step 4: Run test to verify it passes**

Run: `npx vitest run src/services/__tests__/updateChecker.test.ts`
Expected: PASS (all tests pass).

- [x] **Step 5: Commit Task 2**

Run: `git add src/services/ && git commit -m "feat(updater): implement updateChecker service with semver and changelog caching"`

---

### Task 3: Zustand App Update Store (`src/stores/appUpdateStore.ts`)

**Files:**
- Create: `src/stores/appUpdateStore.ts`
- Create: `src/stores/__tests__/appUpdateStore.test.ts`

**Interfaces:**
- Consumes: `@tauri-apps/plugin-updater` (`check`, `Update`), `@tauri-apps/plugin-process` (`relaunch`), `src/services/updateChecker.ts`
- Produces: `useAppUpdateStore` hook with `phase`, `progress`, `availableVersion`, `installUpdate()`, `restartApp()`, `checkForUpdates()`

- [x] **Step 1: Write failing tests in `src/stores/__tests__/appUpdateStore.test.ts`**

```typescript
import { describe, it, expect, beforeEach, vi } from "vitest";
import { useAppUpdateStore } from "../appUpdateStore";

describe("appUpdateStore", () => {
  beforeEach(() => {
    useAppUpdateStore.getState().reset();
  });

  it("starts in idle state", () => {
    const state = useAppUpdateStore.getState();
    expect(state.phase).toBe("idle");
    expect(state.progress).toBeNull();
    expect(state.error).toBeNull();
  });

  it("resets state properly", () => {
    useAppUpdateStore.setState({ phase: "error", error: "Failed" });
    useAppUpdateStore.getState().reset();
    expect(useAppUpdateStore.getState().phase).toBe("idle");
    expect(useAppUpdateStore.getState().error).toBeNull();
  });
});
```

- [x] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/stores/__tests__/appUpdateStore.test.ts`
Expected: FAIL ("Cannot find module '../appUpdateStore'")

- [x] **Step 3: Implement `src/stores/appUpdateStore.ts`**

Create Zustand store handling `idle | checking | downloading | ready | error` phases, downloading chunk progression via `update.downloadAndInstall()`, and relaunch call via `restartApp()`.

- [x] **Step 4: Run test to verify it passes**

Run: `npx vitest run src/stores/__tests__/appUpdateStore.test.ts`
Expected: PASS (all tests pass).

- [x] **Step 5: Commit Task 3**

Run: `git add src/stores/ && git commit -m "feat(updater): implement appUpdateStore Zustand store for update install lifecycle"`

---

### Task 4: UI Presentation (UpdateBadge & UpdateModal)

**Files:**
- Create: `src/components/shell/UpdateBadge.tsx`
- Create: `src/components/shell/UpdateModal.tsx`
- Modify: `src/App.tsx`

**Interfaces:**
- Consumes: `useAppUpdateStore`, `fetchReleasesList`, `calculateVersionDistance`
- Produces: Compact top-bar badge indicating available update, and detailed modal showing markdown commit notes and progress bar

- [x] **Step 1: Implement `UpdateModal.tsx`**

Build dialog displaying:
- Target version and publish date.
- Markdown changelog rendered cleanly with scrollable list.
- Dynamic Action Button:
  - If `phase === 'idle' | 'checking'`: "Download and Install" button.
  - If `phase === 'downloading'`: Active progress bar with percentage (`42%`).
  - If `phase === 'ready'`: Green "Restart AnyLeap to Complete" button.
  - If `phase === 'error'`: Red error banner with "Retry" button.

- [x] **Step 2: Implement `UpdateBadge.tsx`**

Build compact chip displayed in header:
- Hidden when `isLatest == true`.
- Displays `"vX.X.X available"` with subtle glowing dot when update exists.
- Clicking opens `UpdateModal`.

- [x] **Step 3: Integrate into `src/App.tsx`**

Mount `UpdateBadge` in top header bar. Trigger background `checkForUpdates()` on mount inside `useEffect`.

- [x] **Step 4: Verify frontend build & tests**

Run: `npm test && npm run build`
Expected: PASS (vitest 100% pass, vite build succeeds without TypeScript errors).

- [x] **Step 5: Commit Task 4**

Run: `git add src/components/shell/ src/App.tsx && git commit -m "feat(ui): add UpdateBadge and UpdateModal changelog viewer"`

---

### Task 5: Settings View Manual Trigger & Backward Compatibility

**Files:**
- Modify: `src/lib/updateCheck.ts`
- Modify: `src/components/SettingsView.tsx` (or settings panel)

**Interfaces:**
- Consumes: `useAppUpdateStore.checkForUpdates`
- Produces: "Check for Updates" button in Settings that displays explicit user feedback ("Checking...", "You're up to date", or opens UpdateModal).

- [x] **Step 1: Update `src/lib/updateCheck.ts`**

Route legacy `checkForUpdates` function to invoke `useAppUpdateStore.getState().checkForUpdates(true)` so any legacy callers leverage the new store and modal.

- [x] **Step 2: Add Update section in Settings**

Add "Version & Updates" card in settings:
- Displays current version.
- "Check for updates" button triggering manual check.
- Shows status label ("AnyLeap is up to date" or "Update vX.X.X available").

- [x] **Step 3: Run Vitest test suite**

Run: `npm test`
Expected: All tests pass.

- [x] **Step 4: Commit Task 5**

Run: `git commit -am "feat(settings): add manual update check and wire legacy updateCheck adapter"`

---

### Task 6: GitHub Actions CI/CD Release Pipeline

**Files:**
- Modify: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: GitHub tag trigger (`v*`), `secrets.TAURI_SIGNING_PRIVATE_KEY`, `secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
- Produces: Signed Windows NSIS installers, portable ZIPs, `latest.json` updater feed, and automated commit changelog in GitHub Release body

- [x] **Step 1: Add automated tag version sync step to `.github/workflows/release.yml`**

Add PowerShell step before build to sync tag version into `package.json`, `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.toml`.

- [x] **Step 2: Add commit log changelog generator step**

Add step extracting `git log "$prevTag..HEAD"` into markdown bullet list `$body` and outputting as `steps.changelog.outputs.body`.

- [x] **Step 3: Configure `tauri-action` with updater flags and signing secrets**

Update `tauri-action`:
```yaml
env:
  GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
  TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
with:
  tagName: ${{ steps.tag.outputs.tag }}
  releaseName: 'AnyLeap ${{ steps.tag.outputs.tag }}'
  releaseDraft: false
  prerelease: false
  releaseBody: ${{ steps.changelog.outputs.body }}
  uploadUpdaterJson: true
  updaterJsonPreferNsis: true
```

- [x] **Step 4: Commit Task 6**

Run: `git add .github/workflows/release.yml && git commit -m "ci(release): add version sync, commit changelog generator, and tauri updater signing"`

---

## Plan Verification Checklist

- [x] `npm test` passes all tests.
- [x] `cargo check --manifest-path src-tauri/Cargo.toml` succeeds.
- [x] `npm run build` generates `dist/` without errors.
- [x] All 6 tasks have verified commits.
