# SPEC-30: Scroll feel (speed, smooth scrolling, inertia)

> **Status:** Phase 1 implemented (2026-11-19). Phase 2 implemented (2026-11-23; pending manual device check). Phase 3 remains proposed.
> **Scope:** wheel and touchpad input over a mirror window (`src-tauri/src/wheel_swipe.rs` hook thread, new `scroll_feel` module beside it), per-mode persisted settings (`src/lib/modeSettings.ts`), the Phone mirroring section and the control bar **More** popover.
> **Related:** SPEC-28 (per-mode `modeDefaults`, Phone mirroring tabs), ADR-0005, SPEC-24 (input hooks pattern). SPEC-29 is reserved by an open PR, hence the number.
> **Plan:** `docs/superpowers/plans/2026-11-19-spec30-scroll-feel.md`

## 1. Problem (user-tested 2026-11-19)

- scrcpy in `sdk` mouse mode forwards the PC wheel as Android `ACTION_SCROLL`. Android scrolls about **64 dp per wheel notch**, and dp scales with display density.
- Phone Mirror uses the phone's native density (about 400 dpi): about **160 px per notch**.
- Flex virtual displays often use a low density (the user's Flex is `1920x1080/160`): about **64 px per notch**, roughly **2.5x slower** in practice.
- Raising Flex to `/320` made scrolling clearly smoother and faster but **enlarged all UI**. The user wants small UI **and** fast scroll. Scroll speed must be decoupled from display density.
- Each notch is also one discrete jump of 64 to 160 px, which reads as steppy in a long vertical comic or a long article. A touchpad has no momentum at all after the fingers lift.
- The existing wheel swipe feature (`wheel_swipe.rs`) converts a notch into an `adb shell input swipe` fling. It suits short-video pagers but is wrong for continuous reading.

## 2. Goals and non-goals

Goals
- G1. Scroll speed independent of display density, with an `Auto` that makes Flex `/160` feel like `/320` while the UI stays small.
- G2. Optional smooth scrolling: same total distance, delivered in small eased steps.
- G3. Optional inertia for touchpad input.
- G4. All three are per mode (SPEC-28 `modeDefaults`) and independent of each other. Each can be turned off.
- G5. A clear choice between continuous scrolling (reading) and the existing flings (short videos).
- G6. No added CPU or jank on the PC or the phone, and a hook callback that stays non-blocking.

Non-goals
- Changing the scrcpy binary or protocol, or forking scrcpy.
- Per-app scroll profiles (see open questions).
- Touch-gesture emulation of anything but the existing Paged mode.
- macOS and Linux (the hook is Windows only today; other platforms keep native scrcpy behaviour).

## 3. Settings model

Three independent controls plus the style switch, stored per mode inside each `ModeSettings` object (SPEC-28 §5). Existing `modeDefaults` without the new key are filled from defaults at load (spread over `DEFAULT_SCROLL[mode]`); no migration version bump is needed and the legacy keys are untouched.

```ts
type ScrollStyle = "smooth" | "paged";
interface ScrollSettings {
  style: ScrollStyle;                       // "paged" = existing wheel swipe flings
  speed: "auto" | number;                   // manual multiplier, 1..5, step 0.25
  smooth: boolean;                          // Phase 2
  inertia: "off" | "low" | "medium" | "high"; // Phase 3
}
```

| Mode | style | speed | smooth | inertia |
|---|---|---|---|---|
| Mirror | smooth | auto | on (Phase 2) | off |
| Flex | smooth | auto | on (Phase 2) | off |
| Camera | smooth | auto | off | off |
| High fidelity | smooth | auto | off | off |

Until Phase 2 ships `smooth` is stored but inert. High fidelity and Camera default smoothing off because those modes are used for games and live monitoring where latency matters (§9).

The legacy per-device **Mouse wheel swipes** toggle (More popover, command `set_wheel_swipe`) is kept and maps to `style = "paged"` for the running session. A session starts with the mode's stored style; the toggle overrides it until the session ends (same lifetime as today).

## 4. UI

Settings, Phone mirroring, per-mode tab, group **Scrolling** (no new rail section, so SPEC-28 §8 single-home rule holds):

- **Scroll style**: segmented control `Smooth (reading)` / `Paged (short videos)`. Helper: "Smooth scrolls like a normal page. Paged turns each wheel notch into a swipe, for short-video feeds."
- **Scroll speed** (shown when style is Smooth): select `Auto` or `1x` to `5x`. Helper with Auto: "Auto matches the phone's normal feel. Currently 2.5x on this screen." The live number appears only while a session of that mode is running; otherwise the helper says "Matches the phone's normal feel."
- **Smooth scrolling** (Phase 2): switch. Helper: "Glides between wheel notches. Adds a little delay, so turn it off for games."
- **Touchpad momentum** (Phase 3): select `Off / Low / Medium / High`. Helper: "Keeps scrolling briefly after you lift your fingers."
- When style is Paged, speed, smooth scrolling and momentum are disabled with the note "Not used in Paged style."
- The More popover entry is renamed **Paged scrolling** (was "Mouse wheel swipes") and stays a per-session toggle.

Labels follow SPEC-28 §8.5: positive, sentence case, no jargon. Strings go through the existing label table.

## 5. Phase 1: scroll speed

### 5.1 Multiplier

```
auto_multiplier = clamp(native_density / session_density, 1.0, 4.0)
```
then rounded to the nearest 0.25. Manual mode uses the chosen value, clamped to 1.0 to 5.0.

- `native_density`: the device's physical density, the `Physical density:` line of `adb shell wm density` (fallback `densityDpi` of the default display in `dumpsys display`, fallback 400).
- `session_density`: density of the display the mirror shows. Mirror: the `Override density:` line of `wm density` when present, else the physical density. Flex: `densityDpi` of the virtual display parsed from `dumpsys display` (the refresher already re-reads it every ~1.5 s for size).
- Examples: Mirror 400/400 = 1.0x. Flex `/160` on a 400 dpi phone = 2.5x. Flex `/320` = 1.25x. A 480 dpi phone on `/160` = 3.0x. The 4x upper clamp avoids runaway on very low densities (for example `/100` on a 400 dpi phone); the 1x lower clamp means Auto never slows a session below stock behaviour.
- The multiplier is recomputed on the refresher tick when the density changes (user edits `wm density`, window resize for Flex). Smoothing a change: take effect on the next input event, no animation.
- Manual values are not density-aware. They exist for users who want faster or slower than Auto.

### 5.2 Mechanism: options evaluated

**Option A. Re-post a scaled wheel message to the scrcpy window.** The existing low-level hook already swallows `WM_MOUSEWHEEL` over mirror windows. For the scroll style, instead of the fling worker, the callback reads `mouseData` (high word = signed delta), swallows the event, and a worker `PostMessageW`s a new `WM_MOUSEWHEEL` (or `WM_MOUSEHWHEEL`) with `delta * multiplier` to the resolved mirror HWND. SDL's window procedure converts the delta to a precise scroll value (delta / 120 as a float) and scrcpy sends it as `ACTION_SCROLL` with a proportional vertical scroll value.
- Pros: no scrcpy changes, works with the shipped binary, reuses the hook thread, HWND resolution and registry. No hook recursion: `PostMessageW` does not pass through `WH_MOUSE_LL` (only `SendInput` does). Fractional multipliers work because the delta is a continuous quantity. Smooth scrolling and inertia (Phases 2 and 3) are the same mechanism with more messages.
- Cons: relies on SDL reading `WM_MOUSEWHEEL` (already assumed by `wheel_swipe.rs`, only valid for `sdk` mouse mode). scrcpy limits the size of one scroll event, so a posted delta is capped per message (§5.3 splitting). `lParam` must carry screen coordinates and `wParam` the key state (`MK_CONTROL`, `MK_SHIFT`), which we copy from the original event.

**Option B. Send scaled scroll through scrcpy's control channel.** AnyLeap launches scrcpy as a subprocess and the control socket belongs to it. Writing scroll-inject messages ourselves would need either a second control client (the server accepts only one) or replacing the scrcpy client with our own. A large scope increase for no user-visible gain.

**Decision: Option A.** It is the smallest change that reaches the goal, and the same pipeline serves all three phases. Option B is revisited only if a future scrcpy release stops reading `WM_MOUSEWHEEL` (for example by switching to raw input); the symptom would be a scroll that no longer scales, covered by a device check in §11. Open question 1 confirms the precise-scroll path in the bundled scrcpy before implementation.

### 5.3 Event handling rules

- **Vertical wheel** (`WM_MOUSEWHEEL`): scale and re-post.
- **Horizontal wheel** (`WM_MOUSEHWHEEL`, touchpad horizontal scroll or tilt wheels): scale with the same multiplier and re-post. The hook currently ignores it; it must be handled so horizontal scroll does not become slower than vertical.
- **Ctrl+wheel** (zoom in the mirrored app): do not touch. The low-level hook struct has no key state, so read `GetAsyncKeyState(VK_CONTROL)` (non-blocking). If Ctrl, Alt or Win is down, fail open and pass the event through unscaled.
- **Shift+wheel** (horizontal by convention): pass the key state through unchanged and scale once.
- **Not over a mirror window, session disabled, registry locked, or channel full:** fail open (`CallNextHookEx`), as today.
- **Style Paged:** existing path, untouched.
- **Style Smooth with effective speed 1x, smooth off, inertia off:** the hook fails open and does nothing. Scrolling is then identical to stock scrcpy.
- **Remainders:** multiply in `f32` with a per-axis accumulator. The integer part is posted, the fractional remainder carries to the next event. This makes small touchpad deltas (for example 7, 12, 3) scale correctly: 2.5x of 3 is 7.5, posts 7 and carries 0.5, and the long-run sum equals `sum(delta) * multiplier` within 1 unit. The accumulator resets after 250 ms of silence or a change of direction.
- Posted delta is clamped to the `i16` range per message; a larger value is split across messages.
- **Precision touchpads** send many small deltas (often 1 to 30 per message, not multiples of 120). They go through the same accumulator. "Touchpad-like" input (`|delta| % 120 != 0` over the last few events) is detected only for Phase 2 (bypass smoothing, §6) and Phase 3 (inertia eligibility).

### 5.4 Hook and threads

- `mouse_proc` stays non-blocking: `ANY_ENABLED` atomic fast path, `try_lock` on the registry, one `try_send` to the worker, no cross-process calls, no allocation. New fields it reads (`style`, `multiplier`) live in the per-HWND `SwipeTarget`, copied out under the `try_lock`.
- The worker owns the accumulator and `PostMessageW`. `PostMessageW` is asynchronous and cheap, but it never runs on the hook thread.
- A swallowed event cannot be restored, so the callback decides to swallow only after `try_send` succeeds. If the queue is full, the original event passes through and scrolls at 1x (graceful degradation).
- Density and multiplier come from the refresher thread, which already queries `adb`. It gains one `wm density` call at session start and again on a slow tick (about every 5 s for Mirror, every Flex refresh tick for Flex). Results cache in the registry.
- Command `set_scroll_feel(serial, ScrollConfig)` sits beside `set_wheel_swipe`, called on session start and when the user edits settings for the running mode. The frontend sends `{ style, speed_mode: "auto" | "manual", manual: f32, smooth, inertia }`; the backend computes Auto because it owns the density.

## 6. Phase 2: smooth scrolling

Goal: the same total distance as without smoothing, delivered as small steps, with consecutive notches merging into one motion.

**Model.** A per-axis *pending distance* `P` (wheel delta units after the speed multiplier). Each input adds to `P`. A timer drains it:

```
every tick (dt = 8 ms):
    step = P * (1 - exp(-dt / tau))        // exponential ease-out, tau = 35 ms
    emit step (with remainder carry); P -= step
    if |P| < 1 unit: emit P; P = 0         // flush, total preserved
```

- A single notch is delivered in about 100 to 150 ms: with `tau = 35 ms`, 95% of the distance is out in about 3 tau (105 ms) and the flush finishes it by about 130 ms. That is about **12 to 16 steps per notch** (front-loaded, fine tail).
- Consecutive notches add to `P` and continue the same curve, so a fast spin keeps one smooth motion with no per-notch restart. Merging is automatic.
- Total distance is conserved: `sum(emitted) == sum(input) * multiplier` after the flush (remainder carry plus final flush). A property test covers it.
- Easing: exponential ease-out was chosen over a fixed-duration cubic per notch because it merges notches without a discontinuity and needs no per-notch state.
- **Tunables:** `tau` 35 ms (range 20 to 60), tick 8 ms (about 125 Hz), max settle 150 ms, cap on `P` of 20 notches so a runaway spin cannot queue seconds of scrolling.
- **Touchpad bypass:** touchpad-like deltas (§5.3) skip smoothing, since they are already smooth and extra delay feels laggy. They still get the multiplier.
- **Timer:** the worker wakes only while `P != 0` (`recv_timeout(8 ms)` loop, with a 1 ms timer resolution request scoped to active scrolling). At idle it blocks and uses no CPU.
- Default: on for Mirror and Flex, off for Camera and High fidelity (§3).

## 7. Phase 3: inertia

Goal: after a touchpad scroll stops, keep scrolling briefly with decay. Mainly for touchpad.

### 7.1 Options compared

**(a) Synthesized decaying scroll steps.** Track velocity over the last ~80 ms of touchpad deltas. When the stream stops, keep emitting wheel steps with decaying velocity `v(t) = v0 * exp(-t / tau_i)` until `|v| < 0.5 unit/tick`.
- Pros: same pipeline, multiplier and `PostMessageW` path as Phases 1 and 2. Fully under our control: strength setting, instant cancel. Identical on Mirror and Flex. No adb process.
- Cons: the decay is our curve, not the app's fling physics. Apps with their own scroll animation add a second smoothing layer.

**(b) Synthesized touch fling (Android native fling physics).**
- Pros: authentic physics per app.
- Cons: `adb shell input swipe` spawns a process (about 100 to 300 ms), so the fling starts late and cannot be cancelled or retuned mid-flight. Velocity is controllable only through swipe duration. Touch injection can conflict with a real finger and turns a scroll into a page flip in pagers. Fling distance is in px, so it cannot use the density multiplier. Stopping a fling needs another injected touch. This is the same machinery that makes Paged unsuitable for reading.

**Decision: (a).** Local, cancellable, tunable and consistent with Phases 1 and 2. (b) stays Paged's business.

### 7.2 Strength setting

`inertia`: `off | low | medium | high` maps to `tau_i` of **150 / 300 / 500 ms** and velocity gain 0.8 / 1.0 / 1.2. Coast distance is about `v0 * tau_i`, so High glides about 3x farther than Low. Cancel immediately on any new wheel event, mouse button, key press, or the pointer leaving the mirror window. Hard cap on coast duration 1.2 s.

### 7.3 Caveat

Windows precision touchpad drivers often emit their own inertial wheel deltas after the fingers lift. Synthetic inertia on top would double it. Phase 3 therefore starts with a measurement: record the delta stream from the user's touchpad over a lift. If the driver already supplies a tail, Phase 3 reduces to keeping that tail intact under the Phase 1 multiplier and the Phase 2 bypass, and the inertia setting stays hidden. Synthesized inertia ships only when the stream stops abruptly (open question 3).

## 8. Relationship to Paged (wheel swipe)

| | Smooth (reading) | Paged (short videos) |
|---|---|---|
| Mechanism | scaled wheel messages to the scrcpy window | `adb shell input swipe` flings, one per 300 ms cooldown |
| Good for | long vertical comics, articles, settings lists, feeds | one-item-per-swipe feeds that ignore wheel scroll |
| Speed, smooth, inertia | apply | ignored |

Exactly one style is active per session. The hook is shared and dispatches on the target's `style`. Paged keeps its cooldown, Flex `input -d <id>` support and tests unchanged.

## 9. Latency budget and games

| Stage | Budget |
|---|---|
| Hook callback (swallow and `try_send`) | well under 1 ms, never blocks |
| Worker to `PostMessageW` (Phase 1) | under 1 ms added |
| Smooth scrolling (Phase 2) | first step within 8 ms; perceived centre of motion delayed about 35 ms; settled within 150 ms |
| Inertia (Phase 3) | no delay while input is active; coast starts one tick (8 ms) after the stream stops |

Games and other latency-sensitive use: smooth scrolling and inertia add delay by design. Defaults (§3) turn them off in Camera and High fidelity, and the user can turn them off per mode at any time. Speed 1x with both off makes the hook do nothing (§5.3).

## 10. Performance

- The hook callback follows the existing rules in `wheel_swipe.rs`: atomic fast path, `try_lock` registry (fail open), `try_send`, no cross-process calls.
- Idle cost is zero: the smooth/inertia worker blocks when `P == 0` and no coast is active. The extra `wm density` call is one adb round trip every few seconds inside the existing refresher.
- Active cost: at most about 125 tiny `PostMessageW` per axis per second while scrolling. The phone sees a normal `ACTION_SCROLL` stream, far below finger touch rates.
- Measurement (acceptance for Phases 1 and 2): PC CPU of the host and scrcpy while scrolling a long list for 30 s with each feature on and off; phone jank via `adb shell dumpsys gfxinfo <package> framestats` on a system settings screen. Accept: host CPU delta under 1 percentage point and no more janky frames than stock scrcpy.

## 11. Test plan

Unit (Rust, pure functions in a platform-independent submodule so CI runs them on any OS):
- **Multiplier math.** Auto: 400/400 = 1.0, 400/160 = 2.5, 400/320 = 1.25, 480/160 = 3.0, 400/100 clamps to 4.0, 300/400 clamps to 1.0, missing or 0 density falls back to 1.0. Rounding to 0.25. Manual clamp 1 to 5.
- **Density parsing.** `wm density` with and without an override line; `dumpsys display` virtual display block.
- **Accumulator.** Deltas 3, 7, 12 at 2.5x sum to the expected total within 1 unit; reset after 250 ms silence and on direction change; `i16` splitting.
- **Step splitting and merging (Phase 2).** One notch emits at least 8 steps within 150 ms and sums exactly to `120 * multiplier`. Three notches within 60 ms merge and sum exactly. Opposite-direction notches cancel. Property test: `sum(emitted) == sum(input) * multiplier`. Touchpad-like deltas bypass smoothing.
- **Inertia decay (Phase 3).** Velocity from an 80 ms window; decay monotonic; stops under threshold; coast distance within 5% of `v0 * tau_i`; cancel on new input.
- **Hook decision function** (pure): key state, window match, style, multiplier, queue state returns `Swallow` or `PassThrough`. Cases: Ctrl, horizontal, disabled, full queue.
- Frontend (Vitest): defaults per mode, fill-in for `modeDefaults` without the key, controls disabled under Paged, labels contain no third-party names.

Device checks (manual, recorded in the implementing PR):
- **TECNO CM7 (Android 16)** and **Redmi (HyperOS 2, Android 14)**. Use only a long scrolling list inside a system settings screen. Never open personal apps.
- Measure pixels scrolled per notch before and after: screenshot after a fixed number of notches (for example 5) and compare content offset, or read the offset via `dumpsys` where the view exposes it. Compute px/notch.
- Expectations: Mirror about 160 px/notch (1.0x). Flex `/160` with Auto about 160 px/notch, comparable to stock Flex `/320`, while UI stays at `/160` size. Manual 1x on Flex `/160` returns to about 64 px/notch.
- Horizontal scroll scales the same. Ctrl+wheel behaves as before. Paged on a short-video feed behaves exactly as before.
- Feel: with smooth scrolling on, spin quickly; no stutter, no overshoot, no drift after stopping. Off restores stock feel.
- Mirror and Flex each keep their own scroll settings.

## 12. Phasing

1. **Phase 1: scroll speed.** `ScrollSettings` model, Scrolling group UI (style and speed), `set_scroll_feel`, density queries, multiplier, accumulator, horizontal and Ctrl rules, Paged mapping, device checks. Ships alone.
2. **Phase 2: smooth scrolling.** Pending-distance model, timer worker, touchpad bypass, UI switch, default on for Mirror and Flex. Ships alone.
3. **Phase 3: inertia.** Starts with the touchpad delta measurement (§7.3). Ships only if the driver stream needs it.

Each phase ends green: `npx tsc --noEmit`, `npx vitest run`, `cargo test`.

## 13. Risks

| Risk | Mitigation |
|---|---|
| SDL or scrcpy changes how it reads the wheel (raw input) | Device check on each scrcpy bump; fail open and revisit option B |
| Bundled scrcpy ignores fractional precise scroll | Open question 1 gates Phase 1; fall back to integer multipliers with remainder carry |
| Some apps do not use the standard 64 dp factor | Manual speed override; per-app profiles are a non-goal |
| Auto too fast on a very small Flex window | 4x clamp, manual override |
| Smooth scrolling feels laggy | 150 ms settle limit, off switch, tunable `tau` |
| Double inertia with a precision touchpad driver | Measure first (§7.3) |
| Windows removes a slow hook | Callback stays non-blocking (§10); swallow only after `try_send` |
| Real injected wheel input is re-scaled | Scaling only over a resolved mirror window; posted messages never re-enter the hook |

## 14. Open questions

1. **Answered (2026-11-19, bundled scrcpy 5.0.1 / SDL3 on TECNO CM7 Flex `1920x1080/160` Settings list via `uiautomator dump`):** Yes — SDL3 uses precise scroll values. Posted `WM_MOUSEWHEEL` deltas `-120`, `-300` (2.5 notches), and `-60` (0.5 notch) scrolled `192 px`, `480 px` (`2.5x`), and `96 px` (`0.5x`, twice) respectively. Fractional multipliers work continuously; the per-axis `f32` accumulator carries sub-unit remainders into `i16` posted deltas.
2. **Answered (2026-11-19, same spike):** `PostMessageW` to the `SDL_app` HWND with screen coordinates in `lParam` works even when the cursor is outside the window and without foreground focus. Setting `MK_SHIFT` (`0x0004`) in `wParam` alone without physical Shift held still scrolled vertically `192 px`, showing SDL3 reads keyboard modifier state (`GetKeyState`) rather than `wParam` `MK_SHIFT`; we still forward `MK_SHIFT` in `wParam` and handle `WM_MOUSEHWHEEL` directly.
3. What does the user's touchpad driver emit after a lift (decaying tail or abrupt stop)? Decides Phase 3.
4. Should Auto depend on the foreground app? Default no.
5. Should the Auto upper clamp be user-visible?
6. Should Paged be suggested automatically when a pager app is in the foreground? Out of scope.
