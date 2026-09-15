# AnyLeap Development Plan & Specifications

## Overview

This document indexes all architecture specifications, implementation plans, and forward development roadmaps for AnyLeap.

---

## Specifications Directory

| Spec | Title | Status | Scope |
|---|---|---|---|
| **[SPEC-01](specs/SPEC-01-mutex-hardening.md)** | Mutex Hardening & Poison Handling | Ready to implement | Backend crash-proofing, `poison_error` mapping |
| **[SPEC-02](specs/SPEC-02-cross-platform-build.md)** | Cross-Platform Build & Sidecars | Ready to implement | `fetch-binaries.js`, macOS/Linux support |
| **[SPEC-03](specs/SPEC-03-session-lifecycle-and-flex-teardown.md)** | Session Lifecycle & Flex Teardown | Ready to implement | Clean session cleanup, `KEYCODE_BACK` teardown |
| **[SPEC-04](specs/SPEC-04-shared-settings-persistence.md)** | Shared Settings Persistence | Ready to implement | Unified `settings.json` via `tauri-plugin-store` |
| **[SPEC-05](specs/SPEC-05-screen-power-and-encoding-fix.md)** | Screen Power Toggle & Encoding Fix | High Priority | Fix non-working screen-off, PID resolution, `WM_SYSKEYDOWN` |
| **[SPEC-06](specs/SPEC-06-embedded-mirror-window.md)** | Integrated Embedded Mirror Window | High Priority | Replace floating strip with single-window reparented mirror |
| **[SPEC-07](specs/SPEC-07-escrcpy-feature-additions.md)** | Escrcpy Feature Additions | Planned | App Drawer, File/APK Drag & Drop, Nicknames, Clipboard Snap |

---

## Technical Comparison: AnyLeap vs Escrcpy

| Capability | AnyLeap | Escrcpy |
|---|---|---|
| **Window Layout** | Floating overlay strip (SPEC-06 migrates to Embedded Mirror) | Single Inset/Embedded Window |
| **Virtual Display (Flex Mode)** | **Deep**: Auto-parses virtual display ID from logs, dynamically targets navigation input (`-d <id>`), restores density on teardown | **Basic**: Passes `--new-display` argument only; inputs default to physical display 0 |
| **Reels / Shorts Wheel Swipe** | **Native**: Background Windows mouse hook maps wheel delta to Android swipe fling | None (manual mouse drag only) |
| **App Launcher Drawer** | In SPEC-07 | Native drawer supporting direct package launches |
| **File Push / APK Install** | In SPEC-07 | Full drag-and-drop support |
| **Tech Stack** | Rust + Tauri v2 + React (compact native binary) | Node.js + Electron + Vue (larger runtime) |
