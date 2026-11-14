# SPEC-26: "Ink & Gold" App Theme & Interaction Polish

> **Status:** Approved (`2026-11-14`)
> **Scope:** `(settings)` — Replaces the Minimalist Graphite palette (`SPEC-17`) with a flat Ink & Gold design system and design-engineering interaction polish.

## 1. Objective

Replace AnyLeap's current Graphite/cyan palette with a cohesive, flat **Ink & Gold** visual system and refine every interactive surface using design-engineering motion, origin, and accessibility principles. Behaviour, layout structure, and user-facing copy remain unchanged except where theme clarity requires token alignment.

---

## 2. Palette & Semantic Tokens

All theme custom properties are defined in `src/index.css` (`:root`) and exposed to Tailwind v4 via `@theme` so components use semantic token utilities exclusively instead of hardcoded hex or generic palette classes.

### 2.1 Surface Tokens (Flat)
| Token | Hex | Role |
|---|---|---|
| `--ink-950` | `#12141F` | App canvas background |
| `--ink-900` | `#1B1E2E` | Panels, cards, sidebars, device tiles |
| `--ink-800` | `#2A2D40` | Raised controls, inputs, active tabs, hover fills |
| `--ink-700` | `#3A3D52` | Hairline borders, dividers, subtle hover borders |

### 2.2 Text Tokens
| Token | Hex | Contrast vs `--ink-950` / `--ink-900` | Role |
|---|---|---|---|
| `--text-strong` | `#ECEAE3` | 15.23:1 / 13.72:1 | Headings, active labels, primary emphasis |
| `--text` | `#C9CBD6` | 11.35:1 / 10.22:1 | Standard body text, controls |
| `--text-muted` | `#8B8FA3` | 5.72:1 / 5.15:1 | Secondary metadata, section kickers, placeholders |

### 2.3 Accent Tokens (Sparing Usage)
| Token | Hex | Contrast | Role |
|---|---|---|---|
| `--gold-400` | `#D8B46A` | 9.30:1 vs `--ink-950` | Primary accent on dark: primary CTA fill (with `--ink-950` text), active/selected state highlight, `:focus-visible` ring, active edge highlight in the KVM layout canvas, key brand mark moments |
| `--gold-600` | `#A9822F` | 5.17:1 (`--ink-950` text) | Pressed/hover primary CTA state, accent on light surfaces |
| `--mist-300` | `#B7C9D6` | 10.77:1 vs `--ink-950` | Info / secondary highlight (transport badges, secondary telemetry, cursor/peer indicator) |

**Gold restraint rule:** Gold is reserved strictly for primary CTAs, active/selected states, keyboard focus rings, the active shared edge highlight in the KVM layout canvas, and key brand moments (`AppLogo` beam, update download progress/CTA). All other chrome uses neutral Ink tones.

### 2.4 Status Tokens (Non-Neon, `>= 4.5:1` Text Contrast)
| Token | Hex | Surface Pair | Contrast | Role |
|---|---|---|---|---|
| `--status-success` | `#6DB88B` | `--status-success-bg` (`#1B2C28`) / `--ink-950` | 6.18:1 / 7.75:1 | Connected/ready state, synced badges, pass checks |
| `--status-warning` | `#E0A858` | `--status-warning-bg` (`#2A231D`) / `--ink-950` | 7.31:1 / 8.65:1 | Corner deadzones, warnings, unauthorized state |
| `--status-danger` | `#E48282` | `--status-danger-bg` (`#2B1A22`) / `--ink-950` | 6.14:1 / 6.82:1 | Errors, destructive actions, failed checks |
| `--status-danger-solid` | `#B83D3D` | `--text-strong` (`#ECEAE3`) | 4.62:1 | Solid Stop / Confirm Delete buttons |

---

## 3. Flat Surface & Interaction Rules

### 3.1 Flat Colour Purity
- **Prohibited anywhere in `src/`:** CSS/Tailwind gradients (`gradient`, `bg-gradient-*`, `from-*`, `via-*`, `to-*`, `linear-gradient`, `<linearGradient>`), neon glows or coloured shadows (`shadow-[...]`), `drop-shadow`, glassmorphism, and `backdrop-blur*`.
- Elevation uses flat surface stepping (`ink-950` → `ink-900` → `ink-800`), hairline `ink-700` borders, and at most a subtle neutral elevation shadow (`--shadow-elevated: 0 8px 24px rgba(0, 0, 0, 0.36)`) on floating modals/windows.
- Hardcoded zinc/cyan/sky/emerald hex and utility overrides across `src/**` are replaced with semantic `ink-*`, `text-*`, `gold-*`, `mist-*`, and `status-*` tokens.
- The 2D KVM layout canvas blueprint grid uses an SVG pattern instead of CSS `linear-gradient`.

### 3.2 Design-Engineering Interaction Polish
1. **Tactile Press Feedback:** Every pressable button/card uses `transform: scale(0.97)` on `:active:not(:disabled)` with a 160ms custom ease-out (`--ease-out: cubic-bezier(0.23, 1, 0.32, 1)`).
2. **Explicit Transition Properties:** `transition: all` / `transition-all` is forbidden; transitions specify exact properties (`transform`, `opacity`, `background-color`, `border-color`, `color`).
3. **Duration Caps:** Standard UI transitions are `<= 250ms` (140–200ms); modals and drawers are `<= 400ms` (enter 200–240ms, exit faster at 140–160ms).
4. **Origin Awareness:** Popovers/inline panels anchor `transform-origin` at their trigger; centered modals keep `transform-origin: center`.
5. **Natural Scale Entry:** Entering overlays start from `scale(0.95)` + `opacity: 0` (never `scale(0)`).
6. **Zero Keyboard Latency:** Keyboard-initiated actions — specifically the `Ctrl+K` Command Palette (`CommandPalette.tsx`) and shortcut toggles — open, navigate, and close with **0ms animation**.
7. **Pointer-Fine Hover Gating:** Hover transforms and hover-only reveal effects are gated behind `@media (hover: hover) and (pointer: fine)` so touch taps never trigger sticky hover states.
8. **Reduced Motion:** `@media (prefers-reduced-motion: reduce)` drops transform/translation movement while preserving opacity transitions for state comprehension.
9. **No Decorative Loops:** Decorative `animate-pulse`, `animate-ping`, and `animate-bounce` loops on frequently seen surfaces are removed. Only `animate-spin` on genuine active loading/refreshing spinners is retained.

---

## 4. Verification & Quality Gates

- **Token & Contrast Vitest Suite (`src/lib/themeTokens.test.ts`):**
  - Parses `src/index.css` to assert every required Ink, Text, Accent, Status, and Motion CSS variable exists.
  - Computes WCAG 2.1 contrast ratios for all key text/background and button/accent pairs and asserts `>= 4.5:1`.
  - Scans all source files under `src/` to verify zero `gradient` classes/`linear-gradient`/`linearGradient`, zero `backdrop-blur`, zero `drop-shadow`, zero `shadow-[` coloured glows, and zero `transition-all` / `transition: all`.
- **Workspace Gates:**
  - `cargo check --manifest-path src-tauri/Cargo.toml`
  - `cargo test --manifest-path src-tauri/Cargo.toml`
  - `npm test`
  - `npm run build`
- **Manual Inspection Checklist:** Every tab (`Devices`, `Phone`, `Computers`, `Settings`) in dark UI, `:focus-visible` gold rings, `prefers-reduced-motion` behaviour, floating Control/HUD windows.
