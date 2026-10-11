# AGENTS.md — Contributor & AI Agent Guidelines

This document outlines mandatory protocols for AI agents and automated workflows contributing to the AnyLeap repository.

---

## 1. Commit Policy & History Management

### 1.1 Atomic Single-Commit Rule per Feature
- **Always Squash Before Landing**: When completing a specification, task plan, or bounded feature branch, all intermediate/scratch commits must be squashed into a **single clean, atomic commit** before merging into `main`.
- **No Scratch Commits**: Never leave "fix typo", "temp checkpoint", or incremental test step commits in the permanent git history.

### 1.2 Conventional Commit Formatting
All commit messages must strictly adhere to the Conventional Commits specification:
```
<type>(<scope>): <concise description in imperative mood> [(<SPEC-ID>)]
```
- **Allowed Types**:
  - `feat`: New user-facing or architectural capability.
  - `fix`: Bug fix or error resolution.
  - `docs`: Documentation, architecture specs, or plans.
  - `ci`: GitHub Actions workflows or release scripts.
  - `test`: Vitest or Rust test additions.
  - `refactor`: Code reorganization with no behavioral change.
- **Scope Tags**:
  - `(updater)` — Auto-updater, Minisign signing, release feeds.
  - `(kvm)` — Multi-PC virtual KVM, edge leap, input routing.
  - `(mirror)` — Scrcpy display mirroring and container windows.
  - `(camera)` — Android Camera Studio & virtual webcam pipelines.
  - `(deck)` — Studio Deck workstation, presets, and launch matrix.
  - `(settings)` — User preferences, persistence, quality configs.
- **Example**: `feat(updater): implement in-app auto updater, minisign signing, and commit changelog (SPEC-16)`

### 1.3 Strict Confidentiality & Third-Party Anonymity
- **Zero Mentions of External Competitors/Targets**: Never mention proprietary, third-party, or reverse-engineered target software names anywhere in commit titles, descriptions, code comments, or specifications.
- **Generic Architectural Terminology**: Use generic engineering descriptors: "Native Host", "Reference Architecture", "Peer Desktop Protocol", "Hardware Sensor Assembly".
- **Zero Secret Leaks**: Never commit `.key` files, private signing credentials, or environment secrets to git.

### 1.4 Always Update README.md on Feature Completion
- **Mandatory README Sync**: Whenever a new feature, capability, or user-facing workflow is completed, `README.md` must be updated under `## Features` (and status line if bumping version) before the feature is finalized. Never leave new features undocumented.

---

## 2. Quality Gates Before Committing

Every commit must satisfy three non-negotiable verification gates:
1. **Frontend Tests**: `npm test` (Vitest) must pass 100% of test suites.
2. **Backend Rust Check**: `cargo check --manifest-path src-tauri/Cargo.toml` must compile with 0 errors.
3. **TypeScript / Vite Build**: `npm run build` must produce production assets in `dist/` with 0 type errors.

---

## 3. Specification & Planning Workflow

1. **Specs**: All architectural changes or new capabilities must be documented in `docs/specs/SPEC-XX-<title>.md` and approved before implementation.
2. **Plans**: Multi-step implementations must follow a written plan in `docs/superpowers/plans/YYYY-MM-DD-<title>.md`.
3. **Domain Glossary**: Any newly introduced ubiquitous terminology must be recorded in `CONTEXT.md` and ADRs logged in `docs/adr/`.
