# AnyLeap logo guide

## Files
| File | Use |
|---|---|
| `anyleap-symbol.svg` / `anyleap-symbol-reversed.svg` | Full symbol on light (`#A9822F` Deep Gold) / dark (`#D8B46A` Warm Gold) |
| `anyleap-symbol-small.svg` / `anyleap-symbol-reversed-small.svg` | Below 32 px (continuous gold stroke, 2 breaks, no stand/lip/highlight) |
| `anyleap-horizontal.svg`, `anyleap-stacked.svg` (+ `-reversed`) | Lockups with outlined `"AnyLeap"` wordmark |
| `anyleap-wordmark.svg`, `anyleap-wordmark-reversed.svg` | Standalone wordmark on light / dark |
| `anyleap-symbol-mono-black/white/gold/deep-gold.svg` | One-colour print, stamps, embroidery (small-size `evenodd` cut) |
| `anyleap-horizontal-mono-black/white/gold.svg`, `anyleap-stacked-mono-black/white/gold.svg` | One-colour lockups |
| `anyleap-app-icon.svg`, `anyleap-app-icon-small.svg` | App icon on `#1B1E2E` rounded tile (`rx="56.3"`) |
| `anyleap-palette.svg`, `anyleap-palette.png` | Flat colour palette sheet with HEX, RGB, and contrast ratios |
| `tray-16.png`, `tray-32.png` (+ `tray-light-16/32.png`) | Transparent small-cut tray icons (dark / light taskbar) |
| `favicon.*`, `icon-192.png`, `icon-512.png`, `apple-touch-icon.png`, `maskable-512.png`, `site.webmanifest`, `head-snippet.html` | Web icon set |
| `icons/` | Tauri `src-tauri/icons/` ready set (`32x32.png`, `64x64.png`, `128x128.png`, `128x128@2x.png`, `256x256.png`, `icon.png`, `icon-1024.png`, `icon.ico`, `icon.icns`, Windows Store tiles, `tray-16/32.png`) |
| `fonts/` | Plus Jakarta Sans SemiBold (`PlusJakartaSans-SemiBold.ttf`) and SIL OFL 1.1 licence (`OFL.txt`) |
| `presentation/` | Presentation board slides (`slide-01.png`–`slide-05.png` + `board.html`) |
| `family.png`, `16px-check.png` | Suite comparison next to LaciNet and 16/24/32 px pixel audit |

## Colour
| Name | HEX | RGB | vs `#FFFFFF` | vs `#1B1E2E` | Role |
|---|---|---|---|---|---|
| Recess Ink | `#121420` | `18, 20, 32` | `18.31:1` | `1.11:1` | Monitor screen cavity (dark / tile) |
| Deep Ink | `#1B1E2E` | `27, 30, 46` | `16.51:1` | `1.00:1` | Tile background, light screen/depth, light `"Any"` wordmark |
| Slate Dark | `#2D3147` | `45, 49, 71` | `12.79:1` | `1.29:1` | Light-mode bezel & stand front face |
| Slate Base | `#3B3F59` | `59, 63, 89` | `10.29:1` | `1.61:1` | Tile depth flank / light-mode top face |
| Slate Mid | `#4F5575` | `79, 85, 117` | `7.27:1` | `2.27:1` | Lifted tile front bezel & stand |
| Slate Light | `#6A7196` | `106, 113, 150` | `4.76:1` | `3.47:1` | Lifted tile top face & stand base top |
| Warm Gold | `#D8B46A` | `216, 180, 106` | `1.97:1` | `8.38:1` | Accent & `"Leap"` wordmark on dark / navy |
| Deep Gold | `#A9822F` | `169, 130, 47` | `3.55:1` | `4.66:1` | Accent & `"Leap"` wordmark on light / white (`>=3:1`) |
| Pale Blue Highlight | `#B7C9D6` | `183, 201, 214` | `1.70:1` | `9.70:1` | Sparing monitor chin rim & cursor dart core (`>=32 px` only) |

Do not use `#D8B46A` on white or light surfaces; always use `#A9822F` on light (`3.55:1`). Do not use `#B7C9D6` directly on light backgrounds or inside `<32 px` small cuts.

## Rules
- Minimum size: full symbol `32 px`; below `32 px` use `anyleap-symbol-small.svg` / `anyleap-app-icon-small.svg`. Lockup minimum height `24 px`.
- Clear space: at least `1/4` of the symbol height on every side.
- Dark backgrounds: use `-reversed` files. Light backgrounds: use base files (`#A9822F`). One-colour: use `-mono-*` files.
- Strict flat colours only: **no gradients, no shadows, no glows** anywhere.
- Geometry: isometric angles are exact `30°`; symbol footprint stays square (`w:h = 1.00`).
- Do not: recolour the gold outside `#D8B46A` / `#A9822F`, add gradients/shadows/glows, rotate or stretch, outline the symbol, change the wordmark font, or use the 5-segment full cut below `32 px`.

## Wordmark
Plus Jakarta Sans SemiBold (SIL OFL 1.1, vector `<path>` outlines in files — `<text>` is never used; TTF & licence in `fonts/`). `"Any"` in Deep Ink (`#1B1E2E`) on light or `#FFFFFF` on dark; `"Leap"` in Deep Gold (`#A9822F`) on light or Warm Gold (`#D8B46A`) on dark.

## Not done
- No formal trademark clearance search.
- Physical print / Pantone swatch proofing not performed.
