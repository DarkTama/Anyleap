# ADR 0001: Modular DeviceSource Driver Architecture for Multi-PC and Android Support

## Status
Accepted

## Context
AnyLeap currently operates as a dedicated Android-to-PC tool using ADB and scrcpy sidecars.
To expand scope to multi-PC connects (virtual KVM, companion deck, second screen streaming, and multi-device workstation management), AnyLeap must support heterogeneous device types without compromising the existing rock-solid Android workflow.

## Decision
Refactor AnyLeap's backend device management in Rust (`src-tauri/`) and state management in React/TypeScript (`src/`) around an abstract `DeviceSource` trait / interface:
1. `AdbDevice` retains all existing ADB subprocess orchestration (`scrcpy`, pairing, camera studio).
2. `NetworkPcDevice` introduces an embedded Axum + WebSocket daemon managing peer PC discovery, mutual PIN pairing, DXGI desktop capture, and virtual KVM input events.
3. The Studio Deck UI becomes a unified workstation matrix capable of routing controls to both phone and PC targets simultaneously.

## Consequences
### Positive
- Unified user interface for all workstation companions.
- Android and PC subsystems are cleanly decoupled behind common traits.
- Reuses AnyLeap's Tauri 2 + React foundation without requiring a separate standalone app.

### Negative
- Requires Windows-specific API implementations (DXGI, Media Foundation, Raw Input) within the Rust backend.
- Increases backend Rust codebase surface area and testing requirements.
