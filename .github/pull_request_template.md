## Summary
<!-- Concise description of what this PR changes and why. -->

## Spec / Issue
<!-- Reference SPEC-XX and/or #N (e.g., SPEC-16, Closes #12). -->

## Type
<!-- Mark one with [x]. -->
- [ ] `feat` — New user-facing or architectural capability
- [ ] `fix` — Bug fix or error resolution
- [ ] `docs` — Documentation, architecture specs, or plans
- [ ] `ci` — GitHub Actions workflows or release scripts
- [ ] `test` — Vitest or Rust test additions
- [ ] `refactor` — Code reorganization with no behavioral change

## Quality Gates
<!-- All gates in AGENTS.md §2 must pass before requesting review/merge. -->
- [ ] `npm test` (Vitest)
- [ ] `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings`
- [ ] `cargo check --manifest-path src-tauri/Cargo.toml --all-targets --locked`
- [ ] `npm run build` (`tsc && vite build`)
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --locked`

## Manual Testing
### Done
<!-- What was verified manually (include OS/device setup). -->

### Still Needed
<!-- What still requires manual verification (e.g., two-PC KVM scenarios, physical Android device tests). -->

## Checklist
- [ ] Branch has a single squashed Conventional Commit (`<type>(<scope>): <description> [(SPEC-XX)]`, AGENTS.md §1.1–1.2)
- [ ] Documentation synced per AGENTS.md §1.4 (`README.md`, `CONTEXT.md`, `docs/PLAN.md`, `docs/BACKLOG.md`, `docs/adr/`, `docs/specs/` as applicable)
- [ ] No secrets, `.key` files, or private signing credentials committed (AGENTS.md §1.3)
- [ ] No third-party competitor/target product names in code, comments, specs, or commits (AGENTS.md §1.3)
- [ ] Screenshots attached for UI changes (or N/A)
