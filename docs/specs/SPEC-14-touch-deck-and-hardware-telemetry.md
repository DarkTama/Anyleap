# SPEC-14: Touch Deck Macro Controller & Hardware Telemetry

> **Status:** Planned (Phase 3) | Target Version: `v1.2.0`
> Technical Blueprint: Workstation Macro Surface and Hardware Telemetry System

## 1. Overview
Transform connected mobile devices, tablets, or secondary PC windows into an interactive touch macro controller surface (Stream Deck alternative) and live system hardware monitor.

## 2. Technical Architecture

### 2.1 Action Engine (`src-tauri/src/deck/action_runner.rs`)
- **Action Categories**:
  - `Hotkey`: Injects multi-key chords (`Ctrl+Shift+M`, `Win+Alt+R`, etc.) via Win32 `SendInput`.
  - `AppLaunch`: Starts designated executables or passes URI schemes (`explorer.exe`, `spotify:`, `slack:`).
  - `MediaControl`: Audio mute, volume step, next/prev track via virtual multimedia keys (`VK_VOLUME_UP`, `VK_MEDIA_NEXT_TRACK`).
  - `ObsCommand`: Connects to OBS Studio v28+ via OBS WebSocket v5 (`op: 6 Request` for `SetCurrentProgramScene`, `ToggleRecord`, `ToggleVirtualCam`).
  - `SystemCommand`: Lock workstation (`LockWorkStation`), sleep, turn off display.

### 2.2 Action Configuration Schema (`deck-profiles.json`)
```json
{
  "profileId": "default-deck",
  "name": "Production Workstation",
  "rows": 3,
  "columns": 5,
  "buttons": [
    {
      "id": "btn-0-0",
      "label": "Mute Mic",
      "icon": "data:image/png;base64,...",
      "action": {
        "type": "hotkey",
        "keys": ["Ctrl", "Alt", "M"]
      },
      "feedback": {
        "type": "stateful_toggle",
        "activeColor": "#ef4444",
        "inactiveColor": "#10b981"
      }
    }
  ]
}
```

### 2.3 Hardware Telemetry Provider (`src-tauri/src/telemetry/hardware_sensors.rs`)
- **Telemetry Querying**:
  - Primary: Windows Performance Data Helper (PDH) queries for standard CPU % and available memory.
  - Advanced: Native CLR hosting or helper process querying hardware monitor assembly (`LibreHardwareMonitorLib.dll`) for CPU package temperature, GPU core temperature, GPU fan RPM, and VRAM utilization.
- **WebSocket Broadcast (`/ws`)**:
  - Emits telemetry payload at 1000ms intervals:
```json
{
  "type": "hardware_telemetry",
  "timestamp": 1728659500,
  "cpu": { "load": 24.5, "temp": 52.0 },
  "gpu": { "load": 48.2, "temp": 64.0, "vram_used_mb": 4210, "vram_total_mb": 8192 },
  "ram": { "used_mb": 14200, "total_mb": 32768 }
}
```
