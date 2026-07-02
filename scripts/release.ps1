<#
.SYNOPSIS
  Cut a release: tag the current commit as v<version> and push, which
  triggers the GitHub Actions release workflow (.github/workflows/release.yml).

  The version is read from src-tauri/tauri.conf.json - the single source of
  truth (it's what the app reports via getVersion() and what CI bundles).

  Usage: npm run release

  NOTE: keep this file ASCII-only. Windows PowerShell 5.1 parses BOM-less
  .ps1 files as ANSI, so UTF-8 punctuation breaks the script.
#>

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent

$ver = (Get-Content (Join-Path $root 'src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json).version
$tag = "v$ver"

# Guard rails: everything committed, and this version not already released.
if (git -C $root status --porcelain) {
    throw "Working tree is not clean - commit (or stash) before releasing."
}
if (git -C $root tag --list $tag) {
    throw "Tag $tag already exists locally. Bump the version in tauri.conf.json (and package.json, package-lock.json, Cargo.toml) first."
}
git -C $root fetch origin --tags --quiet
if (git -C $root ls-remote --tags origin $tag) {
    throw "Tag $tag already exists on origin. Bump the version first."
}

# Warn if the version manifests disagree (tauri.conf.json still wins).
$pkgVer = (Get-Content (Join-Path $root 'package.json') -Raw | ConvertFrom-Json).version
if ($pkgVer -ne $ver) {
    Write-Warning "package.json is $pkgVer but tauri.conf.json is $ver - did you sync the manifests?"
}

git -C $root tag -a $tag -m "Release $tag"
git -C $root push origin main $tag

Write-Host ""
Write-Host "Pushed $tag - the release workflow is building now:"
Write-Host "  gh run watch            # follow it from the terminal"
Write-Host "  gh run list -w release  # or list recent runs"
