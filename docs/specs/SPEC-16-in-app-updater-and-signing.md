# SPEC-16: In-App Auto Updater, Minisign Release Signing & Release Changelog

> **Status:** Draft / Planned Prerequisite | Target Version: `v0.9.7`
> **Type:** Infrastructure / Bounded Enhancement
> **Architecture Pattern:** OpenTerm Zustand store (`useAppUpdateStore` + `updateChecker.ts`) + Tauri v2 Updater (`updaterJsonPreferNsis`)

## 1. Objective

Provide a seamless, user-transparent in-app update experience matching the OpenTerm architecture:
- Checks GitHub Releases API in background with 30-minute caching.
- Shows rich release descriptions, markdown changelogs, and version distance ("2 versions behind").
- Single-click background download with percentage progress bar inside a Zustand-driven UI.
- Silent background NSIS execution (`updaterJsonPreferNsis: true`) with one-click app relaunch.
- Automated commit changelog generation during GitHub release builds.

---

## 2. GitHub Actions Release Workflow (`.github/workflows/release.yml`)

### 2.1 Automated Tag Version Sync & Commit Log Generation
Before building, workflow extracts tag version and compiles commit changelog:
```powershell
- name: Sync version from git tag
  shell: pwsh
  run: |
    $ver = '${{ github.ref_name }}'.TrimStart('v')
    
    # Sync package.json
    $pkg = Get-Content package.json -Raw | ConvertFrom-Json
    $pkg.version = $ver
    $pkg | ConvertTo-Json -Depth 10 | Set-Content package.json
    
    # Sync src-tauri/tauri.conf.json
    $tconf = Get-Content src-tauri/tauri.conf.json -Raw | ConvertFrom-Json
    $tconf.version = $ver
    $tconf | ConvertTo-Json -Depth 10 | Set-Content src-tauri/tauri.conf.json
    
    # Sync src-tauri/Cargo.toml
    $cargo = Get-Content src-tauri/Cargo.toml -Raw
    $cargo = $cargo -replace '(?m)^version = ".*"', "version = `"$ver`""
    Set-Content src-tauri/Cargo.toml $cargo

- name: Generate release notes from git commits
  id: changelog
  shell: pwsh
  run: |
    $prevTag = git describe --tags --abbrev=0 HEAD^ 2>$null
    if ($prevTag) {
      $log = git log "$prevTag..HEAD" --no-merges --pretty=format:"- %s (%h)"
    } else {
      $log = git log -n 25 --no-merges --pretty=format:"- %s (%h)"
    }
    
    $body = @"
    ## What's Changed
    $log
    
    ### Downloads
    - ``AnyLeap_*_x64-setup.exe`` — Windows installer
    - ``AnyLeap-*-portable-win64.zip`` — Portable archive
    "@
    
    "body<<EOF`n$body`nEOF" | Out-File -FilePath $env:GITHUB_OUTPUT -Append -Encoding utf8
```

### 2.2 Tauri Action Configuration
```yaml
      - name: Build app + create Release (NSIS)
        id: tauri
        uses: tauri-apps/tauri-action@v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
        with:
          tagName: ${{ github.ref_name }}
          releaseName: 'AnyLeap ${{ github.ref_name }}'
          releaseDraft: false
          prerelease: false
          releaseBody: ${{ steps.changelog.outputs.body }}
          uploadUpdaterJson: true
          updaterJsonPreferNsis: true
```

---

## 3. Cryptographic Signing & Bundling

### 3.1 Minisign Key Generation
```bash
npx @tauri-apps/cli signer generate -w ~/.tauri/anyleap.key
```
- **Public Key**: Committed to `src-tauri/tauri.conf.json` under `plugins.updater.pubkey`.
- **Private Key**: Saved to GitHub Secrets as `TAURI_SIGNING_PRIVATE_KEY`.
- **Password**: Saved to GitHub Secrets as `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

### 3.2 `src-tauri/tauri.conf.json`
```json
{
  "bundle": {
    "createUpdaterArtifacts": true,
    "active": true
  },
  "plugins": {
    "updater": {
      "pubkey": "<MINISIGN_PUBLIC_KEY>",
      "endpoints": [
        "https://github.com/DarkTama/Anyleap/releases/latest/download/latest.json"
      ]
    }
  }
}
```

---

## 4. Frontend Architecture (OpenTerm Pattern)

### 4.1 GitHub Release Checker Service (`src/services/updateChecker.ts`)
- Queries GitHub API (`/repos/DarkTama/Anyleap/releases/latest` and `/releases?per_page=10`).
- Parses SemVer (`parseSemver`, `isNewerVersion`).
- Caches responses in `localStorage` for 30 minutes (`CHECK_INTERVAL_MS`).
- Returns rich `ReleaseItem[]` array containing release title, published date, and full markdown changelog `body`.
- `calculateVersionDistance(releases, currentVersion)`: computes how many releases behind and aggregates markdown notes across uninstalled releases.

### 4.2 Zustand App Update Store (`src/stores/appUpdateStore.ts`)
- State: `phase: 'idle' | 'checking' | 'downloading' | 'ready' | 'error'`.
- `progress`: 0.0 to 1.0 (float) streamed from `update.downloadAndInstall()`.
- `installUpdate()`:
  - Invokes `check()` from `@tauri-apps/plugin-updater`.
  - Downloads signed NSIS payload and executes silent install in background.
  - Sets `phase: 'ready'` upon completion.
- `restartApp()`: calls `relaunch()` from `@tauri-apps/plugin-process`.

### 4.3 UI Presentation
- **Header / Settings Badge (`src/components/shell/UpdateBadge.tsx`)**:
  - Unobtrusive badge in top bar: `"v0.9.7 available"`.
  - Clicking opens **`UpdateModal`**:
    - Shows release title, date, and rendered markdown changelog ("What's Changed").
    - Action button transitions:
      - `[Download and Install]` (Idle)
      - `[Downloading 42% ...]` (Progress bar)
      - `[Restart AnyLeap to Complete]` (Ready)

---

## 5. Components & Files to Create / Touch

1. `src-tauri/Cargo.toml`: add `tauri-plugin-updater = "2"`, `tauri-plugin-process = "2"`.
2. `src-tauri/src/main.rs`: register plugins `.plugin(tauri_plugin_updater::Builder::new().build())` and `.plugin(tauri_plugin_process::init())`.
3. `src/services/updateChecker.ts`: release API queries, SemVer parsing, changelog caching.
4. `src/stores/appUpdateStore.ts`: Zustand store managing download phases and relaunch.
5. `src/components/shell/UpdateBadge.tsx`: header indicator.
6. `src/components/shell/UpdateModal.tsx`: release notes viewer with install/restart action.
7. `.github/workflows/release.yml`: version sync, commit notes generation, `uploadUpdaterJson`, `updaterJsonPreferNsis`.

---

## 6. Verification Plan

- **T1: GitHub API Query**: Test `checkLatestRelease` fetches latest release metadata with 30-min cache.
- **T2: SemVer Comparison**: Test `isNewerVersion("0.9.7", "0.9.6") == true`.
- **T3: Store Flow**: Test Zustand store transitions `idle -> checking -> downloading -> ready` with progress events.
- **T4: Silent Relaunch**: Test `restartApp()` calls `relaunch()` without error.
- **T5: Workflow Notes**: Test PowerShell git log generator produces formatted markdown bullet list from commit history.
