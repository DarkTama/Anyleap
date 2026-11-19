# SPEC-30 Scroll Feel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Decouple scroll speed from display density, then add optional smooth scrolling and touchpad inertia, per mirroring mode.

**Architecture:** Pure, platform-independent Rust module `src-tauri/src/scroll_feel/` (multiplier, accumulator, smoother, inertia, hook decision). The existing `wheel_swipe.rs` hook thread dispatches on a per-window `style` and re-posts scaled `WM_MOUSEWHEEL` / `WM_MOUSEHWHEEL` to the scrcpy window. Settings live in `modeDefaults` (SPEC-28).

**Tech Stack:** Tauri 2, Rust (`windows` crate), React 19 + TypeScript, Vitest.

**Spec:** `docs/specs/SPEC-30-scroll-feel.md`.

## Global Constraints
- Hook callback stays non-blocking: atomic fast path, `try_lock` (fail open), `try_send`, no cross-process calls. Swallow only after `try_send` succeeds.
- Ctrl, Alt, Win + wheel always pass through. Horizontal wheel is scaled.
- No third-party or competitor product names anywhere (`AGENTS.md` §1.3). Say "long vertical comic", "short-video feed".
- Device checks use only a system settings list. Never open personal apps.
- Each phase ends green and ships alone: `npx tsc --noEmit`, `npx vitest run`, `cargo test`.

---

## Phase 0: verification spike (gates Phase 1)

### Task 0.1: Confirm wheel path in bundled scrcpy
- [ ] Post a `WM_MOUSEWHEEL` with delta 300 to a mirror HWND from a scratch binary; observe scroll distance (open questions 1 and 2).
- [ ] Record result in SPEC-30 §14. If fractional delta is rounded, plan integer-only multipliers.

## Phase 1: scroll speed

### Task 1.1: Pure math (TDD)
**Files:** create `src-tauri/src/scroll_feel/{mod,speed}.rs`.
- [ ] Tests first: Auto cases (400/400=1, 400/160=2.5, 400/320=1.25, 480/160=3, 400/100→4, 300/400→1, 0→1), 0.25 rounding, manual clamp 1..5.
- [ ] Implement `auto_multiplier`, `effective_multiplier`.
- [ ] Tests and impl for density parsing (`wm density`, `dumpsys display` virtual display `densityDpi`).
- [ ] Tests and impl for the per-axis accumulator (3, 7, 12 at 2.5x; reset after 250 ms; direction change; `i16` split).

### Task 1.2: Hook decision and dispatch
**Files:** modify `src-tauri/src/wheel_swipe.rs`.
- [ ] Pure `decide(key_state, over_mirror, style, multiplier, queue_ok) -> Swallow | PassThrough` with tests (Ctrl, horizontal, disabled, full queue, 1x no-op).
- [ ] Add `WM_MOUSEHWHEEL` to the hook; add `style` and `multiplier` to `SwipeTarget`.
- [ ] Worker applies accumulator and `PostMessageW` to the resolved HWND with original key state and screen coordinates. Paged path unchanged.
- [ ] Refresher reads `wm density` at start and on slow tick; Flex reuses its refresh tick.

### Task 1.3: Command and settings
**Files:** `src-tauri/src/wheel_swipe.rs` (`set_scroll_feel`), `src/lib/modeSettings.ts`, `src/lib/modeSettings.test.ts`, `src/lib/tauri.ts`.
- [ ] `ScrollSettings` type and `DEFAULT_SCROLL` per mode; fill-in at load for older `modeDefaults`. Tests.
- [ ] Send config on session start and on edit for the running mode. The More popover toggle maps to `style = "paged"` for the session.

### Task 1.4: UI
**Files:** `src/components/settings/PhoneMirroringSection.tsx`, `src/lib/controlBarModel.ts`.
- [ ] Scrolling group: Scroll style (Smooth (reading) / Paged (short videos)), Scroll speed (Auto, 1x to 5x). Disabled state under Paged.
- [ ] Rename More entry to Paged scrolling. Tests for labels.

### Task 1.5: Device checks and measurement
- [ ] TECNO CM7 (Android 16) and Redmi (HyperOS 2, Android 14): px per notch in Mirror, Flex `/160` Auto, Flex `/160` 1x, Flex `/320` stock (spec §11). Horizontal, Ctrl+wheel, Paged regression.
- [ ] CPU and `gfxinfo` jank comparison (spec §10).
- [ ] Docs sync (`AGENTS.md` §1.4): README Features, SPEC-30 status, CONTEXT.md.

## Phase 2: smooth scrolling

### Task 2.1: Smoother (TDD)
**Files:** create `src-tauri/src/scroll_feel/smooth.rs`.
- [ ] Tests: one notch, at least 8 steps within 150 ms, exact sum; merging three notches; opposite directions cancel; cap of 20 notches; property test `sum(emitted) == sum(input) * multiplier`; touchpad-like bypass.
- [ ] Implement pending-distance model (`tau` 35 ms, 8 ms tick, flush under 1 unit).

### Task 2.2: Timer worker
- [ ] Worker blocks when idle; `recv_timeout(8 ms)` while pending; scoped 1 ms timer resolution. Verify zero idle CPU.

### Task 2.3: UI and defaults
- [ ] Smooth scrolling switch, default on for Mirror and Flex, off for Camera and High fidelity.
- [ ] Device feel check and CPU/jank measurement.

## Phase 3: inertia

### Task 3.1: Measurement
- [ ] Record the touchpad delta stream across a finger lift. Decide ship, or keep hidden if the driver provides a tail (spec §7.3).

### Task 3.2: Inertia (TDD, only if shipping)
**Files:** create `src-tauri/src/scroll_feel/inertia.rs`.
- [ ] Tests: 80 ms velocity window, monotonic decay, stop threshold, coast within 5% of `v0 * tau_i`, cancel on new input, 1.2 s cap.
- [ ] Implement strengths Low/Medium/High (`tau_i` 150/300/500 ms, gain 0.8/1.0/1.2).
- [ ] UI: Touchpad momentum select. Device check.
