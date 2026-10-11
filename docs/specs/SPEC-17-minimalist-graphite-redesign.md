# SPEC-17: Minimalist Graphite Workstation UI Redesign

> **Status:** Approved Design | Target Version: `v0.9.8`
> **Design Philosophy:** Emil Kowalski Design Engineering (Craft, Taste, Tactile Micro-Interactions) + Linear/Raycast Minimalist Graphite Palette

## 1. Overview & Objective

Redesign AnyLeap's desktop interface from the legacy cluttered blue workstation into a calm, high-contrast, minimalist graphite console.
Provides clear spatial hierarchy, tactile physics on all interactive elements, reduced visual noise, and a dedicated tabbed home for Multi-PC Virtual KVM (`SPEC-12`).

---

## 2. Visual Architecture & Layout (Variant A: Split Workbench)

```
┌────────────────────────────────────────────────────────────────────────────────────────┐
│  AnyLeap v0.9.7     [ Devices ]   [ Computers (KVM) ]   [ Settings ]       [● Live: 0] │
├──────────────────────────┬─────────────────────────────────────────────────────────────┤
│ DEVICE RACK (240px)      │ WORKSPACE STAGE (flex-1)                                    │
│                          │                                                             │
│ ┌──────────────────────┐ │ ┌─────────────────────────────────────────────────────────┐ │
│ │ Pixel 8 Pro      [●] │ │ │ Pixel 8 Pro                         1920x1080 @ 160 DPI │ │
│ │ Wi-Fi · 84% Battery  │ │ └─────────────────────────────────────────────────────────┘ │
│ └──────────────────────┘ │                                                             │
│                          │ VIRTUALIZATION PROFILE (2x2 Grid)                           │
│ ┌──────────────────────┐ │ ┌───────────────────────────┬─────────────────────────────┐ │
│ │ Galaxy S24 Ultra     │ │ │ [●] Desktop Flex          │ [ ] Phone Mirror            │ │
│ │ USB · Offline        │ │ │     Virtual DeX workspace │     1:1 Native portrait     │ │
│ └──────────────────────┘ │ ├───────────────────────────┼─────────────────────────────┤ │
│                          │ │ [ ] Camera Studio         │ [ ] High Fidelity           │ │
│                          │ │     Webcam sensor feed    │     60 FPS · AV1 pipeline   │ │
│                          │ └───────────────────────────┴─────────────────────────────┘ │
│                          │                                                             │
│                          │ [PARAMETER DOCK: Flex DPI presets / Camera Controls]        │
│                          │                                                             │
│ ┌──────────────────────┐ │ ┌─────────────────────────────────────────────────────────┐ │
│ │ + Pair Device        │ │ │ ▶ LAUNCH WORKSTATION                                    │ │
│ └──────────────────────┘ │ └─────────────────────────────────────────────────────────┘ │
└──────────────────────────┴─────────────────────────────────────────────────────────────┘
```

### 2.1 Navigation Structure (`AppHeader.tsx`)
- **Brand Identity**: Left-aligned "AnyLeap" logotype + version chip (`v0.9.7`).
- **Segmented Tabs**:
  - `Devices`: Android phone/tablet workstation (ADB/scrcpy).
  - `Computers`: Multi-PC Virtual KVM link canvas (`SPEC-12`).
  - `Settings`: Direct inline settings and preferences view.
- **Telemetry & Status**:
  - `UpdateBadge`: Subtle chip appearing when GitHub has updates available.
  - Active Session Pill: Emerald live status dot when stream active.

---

## 3. Surface Tokens & Design System

### 3.1 Color Palette
- Canvas Background: `#0c0c0e`
- Surface Cards / Panels: `#141417`
- Active Card / Focus: `#1b1b20`
- Hairline Borders: `#222226` / `rgba(255, 255, 255, 0.07)`
- Focus Borders: `#52525b`
- Text Primary: `#f4f4f5` (zinc-100)
- Text Secondary / Metadata: `#71717a` (zinc-500)
- Primary CTA Button: `#ffffff` background with `#09090b` text
- Live State Indicator: `#10b981` (emerald-500)

### 3.2 Typography Hierarchy
- Header Logotype: 13px bold (-0.02em letter spacing)
- Navigation Labels: 12px medium
- Section Labels: 10px uppercase bold (0.05em letter spacing, zinc-500)
- Body Labels: 12px regular
- Monospace Telemetry: 11px font-mono (zinc-400)

---

## 4. Micro-Interactions & Animation Framework (Emil Principles)

| Element | Motion / Interaction | Value |
| --- | --- | --- |
| Press Feedback | GPU transform scale on active | `active:scale-[0.97]` |
| Custom Easing | Responsive ease-out curve | `cubic-bezier(0.23, 1, 0.32, 1)` |
| Duration Floor | Snappy UI responses | Under 160ms for clicks and state shifts |
| Modal Entrance | Origin-aware scale & fade | `transform: scale(0.97); opacity: 0` ➔ `scale(1); opacity: 1` over 160ms |
| Hover States | GPU border and background shift | Only on `(hover: hover) and (pointer: fine)` |
| Keyboard Execution | Immediate action | 0ms animation on key-press (`Enter` to launch, `Esc` to close) |

---

## 5. Components & Implementation Scope

1. `src/index.css`: Add custom bezier curve `--ease-out: cubic-bezier(0.23, 1, 0.32, 1)` and `.btn-tactile` utility.
2. `src/components/shell/AppHeader.tsx`: Clean top bar with segmented tabs and live session status.
3. `src/components/deck/StudioDeck.tsx`: Restructure to clean Variant A layout.
4. `src/components/deck/DeviceRack.tsx`: Restyle device rows with clean status pips and battery gauges.
5. `src/components/deck/LaunchMatrix.tsx`: Restyle 2x2 mode cards with clean active borders and tactile click physics.
6. `src/components/computers/ComputersView.tsx`: Scaffolding view for Multi-PC Virtual KVM tab.
7. `src/components/SettingsPanel.tsx`: Restyle cards into graphite aesthetic.

---

## 6. Verification & Quality Gates

- `npm test`: 100% of Vitest suites pass.
- `cargo check`: Rust backend builds with 0 errors.
- `npm run build`: Vite build passes with 0 TypeScript errors.
- Visual inspection: Active press scale (`0.97`) feels snappy on click. No layout shifting on mode select.
