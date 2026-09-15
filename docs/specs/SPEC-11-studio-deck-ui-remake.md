# Implementation Plan: Studio Deck UI Remake & Visual Redesign

> **Goal:** Remake AnyLeap's entire user interface from a generic settings form into "Studio Deck" — a dedicated hardware workstation dashboard tailored for Android virtualization, DeX-like flex displays, and embedded mirroring. Follows the `frontend-design` skill principles.

---

## 1. Design System & Personality ("Studio Deck")

### The Subject Matter
AnyLeap is not a SaaS dashboard; it is a high-performance **device bridge and display virtualization console**. The visual identity reflects hardware precision: instrument panels, broadcast switchers, and calibrated studio decks.

### Core Visual Tokens
- **Canvas / Background**: `#0a0c10` (deep basalt obsidian, not muddy grey or tinted cream).
- **Surface Panels**: `#12151d` with crisp 1px structural borders `#1e2330`.
- **Primary Signal Accent**: `#38bdf8` (precision electric cyan for active connections and primary action triggers).
- **Status Indicators**:
  - Connected: `#10b981` (emerald beacon)
  - Pairing / Wireless Discovery: `#f59e0b` (amber beacon)
  - Error / Offline: `#ef4444` (ruby beacon)
- **Typography**: 
  - Standard UI: Clean modern geometric sans (`Inter`, `system-ui`).
  - Numbers & Telemetry: Tabular figures (`font-mono` / `tabular-nums`) for bitrates, resolutions, FPS, and battery percentages.
- **Micro-Interactions**:
  - Real tactile clicks with 0.98 scale depression.
  - No slow, floating fade-ins. Instant visual feedback with subtle glowing borders on active modes.

---

## 2. Layout Structure: Studio Deck

```
+---------------------------------------------------------------------------------------+
|  ANYLEAP STUDIO DECK                           [ + Wireless Pair ]  [ Settings ]      |
+------------------------------------+--------------------------------------------------+
|  DEVICE RACK (280px)               |  LAUNCH MATRIX & WORKBENCH                       |
|                                    |                                                  |
|  +------------------------------+  |  Choose Launch Profile:                          |
|  | [O] TECNO CAMON 30 Pro       |  |  +--------------------+  +--------------------+  |
|  |     "Work Phone" (Rename)    |  |  | [Desktop Flex]     |  | [Phone Mirror]     |  |
|  |     192.168.1.40:44085       |  |  | DeX virtual desktop|  | 1:1 portrait feed  |  |
|  |     Wi-Fi 5GHz · 82% Battery |  |  | Custom DPI scaling |  | Touch input & keys |  |
|  |     [ Disconnect ]           |  |  +--------------------+  +--------------------+  |
|  +------------------------------+  |  +--------------------+  +--------------------+  |
|                                    |  | [Camera Studio]    |  | [High-Fidelity]    |  |
|  +------------------------------+  |  | Phone as HD webcam |  | 60fps / 16Mbps AV1 |  |
|  | [ ] Galaxy Tab S8 (Offline)  |  |  +--------------------+  +--------------------+  |
|  +------------------------------+  |                                                  |
|                                    |  FLEX DPI SCALE VISUALIZER                       |
|                                    |  Density: [ 160 DPI ] - Smaller Desktop Icons    |
|                                    |  +--------------------------------------------+  |
|                                    |  | Preview:  [A] [B] [C] (Live scaled icons)  |  |
|                                    |  +--------------------------------------------+  |
|                                    |                                                  |
|                                    |  [  LAUNCH EMBEDDED MIRROR  ]                    |
+------------------------------------+--------------------------------------------------+
```

---

## 3. Key Components to Build

### 1. `DeviceRack.tsx` (Left Column)
- Displays all USB and paired wireless devices as hardware modules.
- Live battery level and connection transport badge.
- Inline nickname editing with auto-save.
- One-click connect / disconnect / pair actions.

### 2. `LaunchMatrix.tsx` (Right Column - Top)
- 4 tactile mode cards with active state highlight:
  1. **Desktop Flex**: Virtual display, DeX mode, custom DPI, taskbar toggle.
  2. **Phone Mirror**: Low latency portrait mirror, touch markers, full control.
  3. **Camera Studio**: Camera source, torch toggle, zoom knobs.
  4. **High Fidelity**: 16Mbps AV1/H.265 audio/video stream.

### 3. `DpiVisualizer.tsx` (Right Column - Middle)
- Interactive slider & preset buttons (`120 - Compact`, `160 - Desktop`, `240 - Default`, `320 - Touch`).
- **Live Preview Window**: Displays mock app icons and desktop taskbar dynamically sizing as the user drags the DPI slider or clicks presets.
- Clear explanatory copy: *"Lower DPI = smaller icons and more desktop screen space. Higher DPI = larger touch-friendly UI."*

### 4. `StageWindow.tsx` (Embedded Mirror Window Overhaul)
- Repaired title bar: working drag handle, minimize, maximize/unmaximize, and close buttons.
- Integrated right-docked precision toolbar:
  - Android navigation (Back, Home, Recents).
  - Hardware power toggle (toggles physical screen off while keeping mirror stream live).
  - Flex Taskbar toggle button.
  - App Launcher drawer modal.
  - Instant screenshot button.
  - Direct Windows clipboard image paste trigger (`Ctrl+V` or camera/image icon).
