# Minimalist Graphite Workstation UI Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Redesign AnyLeap's desktop interface into a calm, high-contrast, minimalist graphite workstation console with tactile press physics, clean navigation tabs, and a home for Multi-PC Virtual KVM.

**Architecture:** Refactor the UI shell around a top-level command bar with segmented tabs (`devices`, `computers`, `settings`). The main devices view uses a split workbench (Variant A): left column contains a quiet device rack with battery telemetry, right column contains an active telemetry header, 2x2 mode launch matrix, dynamic parameter dock, and high-contrast launch trigger. Styling adheres to Emil Kowalski design engineering principles using custom easing curves and tactile active scale feedback.

**Tech Stack:** React 19 · TypeScript · Tailwind v4 · Lucide React · Zustand · Vitest

**Spec:** `docs/specs/SPEC-17-minimalist-graphite-redesign.md`

## Global Constraints

- Preserve all existing functionality: Flex display DPI scaling, Phone mirror, Camera Studio launch, Hi-Fi AV1 presets, wireless pairing, and auto-updater.
- Strict color tokens: canvas `#0c0c0e`, cards `#141417`, active surface `#1b1b20`, hairline borders `#222226` / `rgba(255,255,255,0.07)`, primary CTA `#ffffff`.
- Micro-interactions: all interactive buttons and cards must feel responsive with `transition: transform 140ms cubic-bezier(0.23, 1, 0.32, 1)` and `active:scale-[0.97]`.
- All tests must pass with 100% green on `npm test` and build clean on `npm run build`.

---

## File Structure

```
src/
├── index.css                               # Modified: CSS design tokens, custom easing, tactile utilities
├── App.tsx                                 # Modified: top-level activeTab state, shell layout
├── components/
│   ├── shell/
│   │   ├── AppHeader.tsx                   # Created: brand, version chip, segmented tabs, session badge
│   │   ├── UpdateBadge.tsx                 # Existing: updater notification badge
│   │   └── UpdateModal.tsx                 # Existing: release changelog modal
│   ├── computers/
│   │   └── ComputersView.tsx               # Created: Multi-PC Virtual KVM scaffolding canvas
│   ├── deck/
│   │   ├── StudioDeck.tsx                  # Modified: Variant A split workbench restructure
│   │   ├── DeviceRack.tsx                  # Modified: graphite device rows with battery/status pips
│   │   ├── LaunchMatrix.tsx                # Modified: 2x2 mode cards with active border shift
│   │   └── DpiVisualizer.tsx               # Modified: restyle to graphite theme
│   └── SettingsPanel.tsx                   # Modified: restyle to graphite cards
```

---

### Task 1: CSS Design Tokens & Micro-Interaction Utilities

**Files:**
- Modify: `src/index.css`

**Interfaces:**
- Produces: CSS variables `--ease-out`, `--canvas-bg`, `--surface-card`, `--surface-active`, `--border-subtle`, and `.btn-tactile` class.

- [x] **Step 1: Add design tokens to `src/index.css`**

Add root variables and tactile button utility:
```css
:root {
  --ease-out: cubic-bezier(0.23, 1, 0.32, 1);
  --canvas-bg: #0c0c0e;
  --surface-card: #141417;
  --surface-active: #1b1b20;
  --border-subtle: rgba(255, 255, 255, 0.07);
  --border-focus: #52525b;
}

.btn-tactile {
  transition: transform 140ms var(--ease-out), background-color 140ms ease, border-color 140ms ease;
}

.btn-tactile:active {
  transform: scale(0.97);
}
```

- [x] **Step 2: Verify build**

Run: `npm run build`
Expected: SUCCESS

- [x] **Step 3: Commit Task 1**

Run: `git commit -am "style: define graphite tokens, custom easing, and tactile utilities in index.css"`

---

### Task 2: Navigation Shell & Header (`AppHeader.tsx`)

**Files:**
- Create: `src/components/shell/AppHeader.tsx`
- Modify: `src/App.tsx`

**Interfaces:**
- Consumes: `useAppStore.sessions`, `useAppUpdateStore.currentVersion`
- Produces: `activeTab: 'devices' | 'computers' | 'settings'` navigation header.

- [x] **Step 1: Implement `src/components/shell/AppHeader.tsx`**

Build clean top bar component:
- Left: "AnyLeap" logotype + `v{currentVersion}` chip.
- Center: Segmented tab switcher (`Devices`, `Computers (KVM)`, `Settings`) with tactile clicks.
- Right: `UpdateBadge`, session live pill (`● Live: N` in emerald when streaming, `STANDBY` when idle).

- [x] **Step 2: Wire `AppHeader` into `src/App.tsx`**

Introduce `activeTab` state in `App.tsx`. Render `AppHeader` at top of view and switch main body content based on `activeTab`.

- [x] **Step 3: Verify build & tests**

Run: `npm test && npm run build`
Expected: PASS

- [x] **Step 4: Commit Task 2**

Run: `git add src/components/shell/AppHeader.tsx src/App.tsx && git commit -m "feat(ui): add AppHeader with segmented navigation tabs"`

---

### Task 3: Multi-PC Tab Scaffolding (`ComputersView.tsx`)

**Files:**
- Create: `src/components/computers/ComputersView.tsx`

**Interfaces:**
- Produces: Visual scaffold for `SPEC-12` Multi-PC Virtual KVM.

- [x] **Step 1: Implement `src/components/computers/ComputersView.tsx`**

Build clean graphite placeholder layout:
- Header: "Computer Link (Virtual KVM)" with descriptive metadata.
- Spatial 2D canvas placeholder showing two linked monitors with edge leap indicator.
- Feature checklist ("Seamless Edge Leap", "Shared Clipboard", "mDNS Auto-Discovery").
- Link to pairing action.

- [x] **Step 2: Verify build & tests**

Run: `npm test && npm run build`
Expected: PASS

- [x] **Step 3: Commit Task 3**

Run: `git add src/components/computers/ComputersView.tsx && git commit -m "feat(ui): add ComputersView KVM tab scaffolding"`

---

### Task 4: Minimalist Device Rack Refactor (`DeviceRack.tsx`)

**Files:**
- Modify: `src/components/deck/DeviceRack.tsx`

**Interfaces:**
- Consumes: `devices`, `savedDevices`, `sessions`, `selectedSerial`, `onSelectDevice`
- Produces: Sleek graphite device list with emerald status pips, battery gauge, and "+ Wireless Pair" button.

- [x] **Step 1: Restyle device items in `src/components/deck/DeviceRack.tsx`**

Apply graphite styling:
- Outer container: `bg-[#0e0e11] border-r border-[#1f1f23] w-64`.
- Active row: `bg-[#1c1c21] border-[#3f3f46]` with white text.
- Inactive row: `bg-[#131316] border-[#1f1f23] text-zinc-400` with subtle hover.
- Add `.btn-tactile` active press feedback.
- Clean battery meter with percentage text.

- [x] **Step 2: Verify build & tests**

Run: `npm test && npm run build`
Expected: PASS

- [x] **Step 3: Commit Task 4**

Run: `git commit -am "refactor(ui): restyle DeviceRack with graphite tokens and tactile rows"`

---

### Task 5: Launch Matrix 2x2 Grid Refactor (`LaunchMatrix.tsx`)

**Files:**
- Modify: `src/components/deck/LaunchMatrix.tsx`

**Interfaces:**
- Consumes: `currentMode`, `onSelectMode`
- Produces: 2x2 mode card switchboard (Desktop Flex, Phone Mirror, Camera Studio, High Fidelity).

- [x] **Step 1: Restyle mode cards in `src/components/deck/LaunchMatrix.tsx`**

Refactor card appearance:
- Container: `grid grid-cols-2 gap-3`.
- Active card: `bg-[#1a1a20] border-[#52525b] shadow-sm` with active radio dot.
- Inactive card: `bg-[#131316] border-[#222226] text-zinc-400 hover:border-zinc-700`.
- Add `.btn-tactile` for snappy `scale(0.97)` on click.
- Clean typography: bold mode title, subtitle, and feature bullets.

- [x] **Step 2: Verify build & tests**

Run: `npm test && npm run build`
Expected: PASS

- [x] **Step 3: Commit Task 5**

Run: `git commit -am "refactor(ui): restyle LaunchMatrix into clean 2x2 graphite switchboard"`

---

### Task 6: Studio Deck Stage & CTA Integration (`StudioDeck.tsx`, `DpiVisualizer.tsx`)

**Files:**
- Modify: `src/components/deck/StudioDeck.tsx`
- Modify: `src/components/deck/DpiVisualizer.tsx`

**Interfaces:**
- Consumes: `activeDevice`, `currentMode`, `handlePrimaryLaunch`
- Produces: Complete Variant A split workbench stage with parameter dock and primary launch CTA.

- [x] **Step 1: Refactor workbench layout in `src/components/deck/StudioDeck.tsx`**

Restructure layout:
- Remove redundant headers now moved to `AppHeader`.
- Stage header: Active device model, serial, and live telemetry chip (`1080p @ 160 DPI`).
- Dynamic dock: DPI slider for Flex mode, lens controls for Camera mode.
- Primary CTA: High-contrast `#ffffff` block (`[ ▶ Launch Desktop Flex ]`) with `btn-tactile`. When active, switches to red stop button.

- [x] **Step 2: Restyle `src/components/deck/DpiVisualizer.tsx`**

Update slider and preset chips to match graphite theme.

- [x] **Step 3: Verify build & tests**

Run: `npm test && npm run build`
Expected: PASS

- [x] **Step 4: Commit Task 6**

Run: `git commit -am "feat(ui): complete StudioDeck Variant A split workbench with high-contrast CTA"`

---

### Task 7: Settings Panel Graphite Restyle (`SettingsPanel.tsx`)

**Files:**
- Modify: `src/components/SettingsPanel.tsx`

**Interfaces:**
- Consumes: `settings`, `controlConfig`, `appPrefs`
- Produces: Minimalist graphite settings view.

- [x] **Step 1: Update Card and Input styling in `src/components/SettingsPanel.tsx`**

Restyle cards:
- Replace blue-tinted card backgrounds with `bg-[#141417] border-[#222226]`.
- Update dropdown selects and toggle switches to match dark graphite theme.

- [x] **Step 2: Verify build & tests**

Run: `npm test && npm run build`
Expected: PASS

- [x] **Step 3: Commit Task 7**

Run: `git commit -am "refactor(ui): restyle SettingsPanel with unified graphite surface tokens"`

---

### Task 8: Verification, Test Suite & README Sync

**Files:**
- Modify: `README.md`

**Interfaces:**
- Produces: Verified production build, passing test suite, and updated `README.md`.

- [x] **Step 1: Run Vitest test suite**

Run: `npm test`
Expected: 100% pass (all 35 tests pass).

- [x] **Step 2: Run production build**

Run: `npm run build`
Expected: 0 errors.

- [x] **Step 3: Update `README.md`**

Update `README.md` features to highlight the new Minimalist Graphite Workstation interface.

- [x] **Step 4: Squash feature commits into single atomic commit per `AGENTS.md`**

Squash tasks into:
`feat(ui): implement minimalist graphite workstation redesign (SPEC-17)`

---

## Plan Verification Checklist

- [x] `npm test` passes 100% of test files.
- [x] `cargo check --manifest-path src-tauri/Cargo.toml` passes with 0 errors.
- [x] `npm run build` succeeds.
- [x] All interactive buttons exhibit snappy `active:scale-[0.97]` tactile press.
- [x] Segmented tabs switch cleanly between Devices, Computers, and Settings.
- [x] All commits squashed into single atomic feature commit per `AGENTS.md`.
