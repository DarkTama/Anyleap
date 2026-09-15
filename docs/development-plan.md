# AnyLeap Development Plan & Specifications

## Overview

This document indexes all architecture specifications, implementation plans, and forward development roadmaps for AnyLeap.

---

## Specifications Directory

| Spec | Title | Status | Scope |
|---|---|---|---|
| **[SPEC-01](specs/SPEC-01-mutex-hardening.md)** | Mutex Hardening & Poison Handling | Complete | Backend crash-proofing, `poison_error` mapping |
| **[SPEC-02](specs/SPEC-02-cross-platform-build.md)** | Cross-Platform Build & Sidecars | Complete | `fetch-binaries.js`, macOS/Linux support |
| **[SPEC-03](specs/SPEC-03-session-lifecycle-and-flex-teardown.md)** | Session Lifecycle & Flex Teardown | Complete | Clean session cleanup, `KEYCODE_BACK` teardown |
| **[SPEC-04](specs/SPEC-04-shared-settings-persistence.md)** | Shared Settings Persistence | Complete | Unified `settings.json` via `tauri-plugin-store` |
| **[SPEC-05](specs/SPEC-05-screen-power-and-encoding-fix.md)** | Screen Power Toggle & Encoding Fix | Complete | PID window resolution, synced toggle state |
| **[SPEC-06](specs/SPEC-06-embedded-mirror-window.md)** | Integrated Embedded Mirror Window | Complete | Single-window reparented mirror + right sidebar |
| **[SPEC-07](specs/SPEC-07-escrcpy-feature-additions.md)** | Escrcpy Feature Additions | Complete | App Drawer, File/APK Drag & Drop, Nicknames, Screenshot |
| **[SPEC-10](specs/SPEC-10-embedded-flex-fixes-and-clipboard.md)** | Embedded Mirror Fixes, Flex & Clipboard | Complete | Working window drag & buttons, child HWND screen-off, flex taskbar & shortcuts, clipboard image paste |
| **[SPEC-11](specs/SPEC-11-studio-deck-ui-remake.md)** | Studio Deck UI Remake | Complete | Grounded in `frontend-design`: Device Rack, Mode Matrix, Live DPI Scale & Icon preview |

---

## Technical Comparison: AnyLeap vs Escrcpy

| Capability | AnyLeap | Escrcpy |
|---|---|---|
| **Window Layout** | Integrated Embedded Mirror (Win32 reparented single window) | Single Inset/Embedded Window |
| **Virtual Display (Flex Mode)** | **Deep**: Auto-parses virtual display ID from logs, targets navigation input (`-d <id>`), flex taskbar toggle, density restore | **Basic**: Passes `--new-display` argument only; inputs default to physical display 0 |
| **Reels / Shorts Wheel Swipe** | **Native**: Background Windows mouse hook maps wheel delta to Android swipe fling | None (manual mouse drag only) |
| **App Launcher Drawer** | Native drawer supporting direct package launches | Native drawer |
| **File Push / APK Install** | Integrated drag-and-drop support | Integrated drag-and-drop support |
| **Tech Stack** | Rust + Tauri v2 + React (compact native binary) | Node.js + Electron + Vue (larger runtime) |
