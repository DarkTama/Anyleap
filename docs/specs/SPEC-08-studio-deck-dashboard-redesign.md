# Implementation Plan: Studio Deck Dashboard Redesign

> **Goal:** Transform AnyLeap's main interface from a generic utility form into "Studio Deck" — a dedicated hardware bridge dashboard featuring visual device cards, real-time wireless telemetry, and mode-based one-click launch presets.

**Architecture:**
- **Visual Token System:**
  - Canvas: `#0c0d10` (deep obsidian).
  - Card/Panel surfaces: `#14161f` with subtle 1px border `#232738`.
  - Signal accents: `#38bdf8` (electric cyan), `#10b981` (active emerald), `#f59e0b` (pairing amber).
  - Typography: Clean sans with tabular figures for latency, bitrate, and resolution readouts.
- **Two-Pane Studio Layout:**
  1. **Left Deck (Device Sidebar - 280px):**
     - Live device cards with model name, custom nickname, connection transport (USB 3.0 / 5GHz Wi-Fi), IP/port, battery level, and signal quality.
     - Quick "Pair Device" trigger with animated QR beacon.
  2. **Right Deck (Launch Matrix & Preset Studio):**
     - **Mode Cards:**
       - *Desktop Flex (DeX)*: Virtual display, custom DPI preset (160/200/240), taskbar toggle, auto PC resolution match.
       - *Phone Mirror*: Low latency portrait mirror, 60fps, touch indicators.
       - *Content Studio*: High bitrate (16Mbps), H.265/AV1, audio capture enabled.
       - *Webcam Mode*: Video source camera (front/back), torch toggle, clean video feed.
     - **Quick Tuning Drawer:**
       - Collapsible bottom drawer for advanced knobs (buffer sizes, codecs, stay-awake) with visual impact badges.

**Tech Stack:** React 19, Tailwind CSS, Lucide Icons, Zustand (`useAppStore`), Tauri Shell & ADB APIs.

---

## Global Constraints

- Do not break existing saved device data in `settings.json`.
- Preserve fast launch speeds (< 300ms from click to scrcpy spawn).
- Responsive down to 720p monitor heights without clipping.

---

## File Changes

- Create: `src/components/deck/DeviceCard.tsx`
- Create: `src/components/deck/ModeSelector.tsx`
- Create: `src/components/deck/StudioDashboard.tsx`
- Modify: `src/App.tsx`
- Modify: `src/lib/types.ts`

---

## Detailed Tasks

### Task 1: Add Launch Mode Presets to `types.ts`

**Files:**
- Modify: `src/lib/types.ts`

**Implementation:**
Define predefined launch profiles:
```typescript
export type StudioMode = "desktop-flex" | "phone-mirror" | "content-studio" | "webcam";

export interface StudioProfile {
  id: StudioMode;
  name: string;
  tagline: string;
  icon: string;
  settings: Partial<CoreSettings>;
}
```

---

### Task 2: Build Visual Device Card Component (`DeviceCard.tsx`)

**Files:**
- Create: `src/components/deck/DeviceCard.tsx`

**Features:**
- Real-time battery status (`adb shell dumpsys battery`).
- Wi-Fi signal strength indicator.
- Inline 1-click nickname editor.
- Connect / Disconnect / Quick Mirror status badges.

---

### Task 3: Build Mode Selector Matrix (`ModeSelector.tsx`)

**Files:**
- Create: `src/components/deck/ModeSelector.tsx`

**Features:**
- Interactive selectable cards with visual previews.
- Live DPI scaling slider with the mock app icon preview.
- One-click launch action that initiates scrcpy with the active profile.

---

### Task 4: Integrate Studio Deck into `App.tsx`

**Files:**
- Create: `src/components/deck/StudioDashboard.tsx`
- Modify: `src/App.tsx`

**Implementation:**
Replace the flat tab interface with the unified Studio Deck dashboard when running in main window mode.
