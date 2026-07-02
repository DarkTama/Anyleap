# Releasing AnyLeap

The release workflow ([.github/workflows/release.yml](../.github/workflows/release.yml))
builds the NSIS installer + portable ZIP and publishes a GitHub Release.

**It does NOT run on ordinary pushes to `main`** — only on:

1. a pushed tag matching `v*` (the normal path), or
2. a manual **Run workflow** from the Actions tab (`workflow_dispatch`).

If you push a commit and see no Actions run, that's by design — cut a release
with one of the methods below.

## Method 1 — `npm run release` (recommended)

```
npm run release
```

[scripts/release.ps1](../scripts/release.ps1) reads the version from
`src-tauri/tauri.conf.json`, tags the current commit as `v<version>`, and
pushes `main` + the tag. Guard rails: refuses on a dirty working tree or if
the tag already exists (local or origin), and warns if `package.json`
disagrees with `tauri.conf.json`.

## Method 2 — manual tag

```
git tag -a v0.4.0 -m "Release v0.4.0"
git push origin main v0.4.0
```

## Method 3 — GitHub UI, no terminal

Actions tab → **release** → **Run workflow** → branch `main`. The workflow
derives the tag from `tauri.conf.json` (`v<version>`), creates it at HEAD,
and releases under it. Same result as Method 1, useful from a browser.

## Before releasing: bump the version

Keep all four manifests on the same version:

| File | Used by |
|------|---------|
| `src-tauri/tauri.conf.json` | **Source of truth** — app `getVersion()`, update check, CI tag/ZIP name |
| `package.json` + `package-lock.json` | npm |
| `src-tauri/Cargo.toml` (+ `Cargo.lock` via `cargo check`) | Rust crate |

## Watching / retrying a run

```
gh run watch              # follow the current run
gh run list -w release    # recent release runs
gh run rerun <run-id>     # retry a failed run
```

A failed run can be retried without retagging. To redo a release from
scratch: delete the release and tag on GitHub (`gh release delete v0.4.0
--cleanup-tag`), then release again.
