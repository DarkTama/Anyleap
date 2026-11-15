# ADR 0004: Continuity Transfers, Audio Bridge, and Experimental Drag & Drop

## Status
Accepted

## Context
SPEC-25 (Continuity) adds cross-PC file copy-paste (`CF_HDROP`), cross-PC file drag-and-drop, a Linked→Main audio bridge with optional Linked-speaker mute, and session/link handoff on top of the authenticated QUIC transport (`anyleap-kvm/1`, ADR-0003). These capabilities introduce bulk-transfer scheduling, real-time audio datagrams, Windows WASAPI loopback/mute lifecycle hazards, and OLE drag-and-drop edge cases that cannot easily be changed once peers depend on the wire protocol and settings schema.

## Decision
1. **Unidirectional stream staging, clip prefetch limit, and offer/cancel packets (`SPEC-25a`):**
   - Cross-PC file copy-paste announces metadata first via `ClipFileOffer` and only streams files immediately when total size is within `stagingLimitMb` (100 MB default); larger offers wait for an explicit **Fetch** request (`ClipFileFetch`), and either side may abort an active transfer with `TransferCancel`.
   - File payloads travel on dedicated unidirectional QUIC streams (`StreamHeader::DragFile` / `ClipFile` / `OpenFile`) into a sanitized staging directory before committing `CF_HDROP` or opening, avoiding head-of-line blocking on the control stream.
2. **Tagged raw PCM QUIC datagrams for audio (`SPEC-25b`):**
   - Audio frames (48 kHz, 16-bit LE stereo PCM, 5 ms / 960 bytes per frame) are sent as unreliable QUIC datagrams prefixed with tag `0x41` (`AudioFrame`), keeping the existing 12-byte mouse-delta datagram format untouched and avoiding a new codec dependency.
3. **Process-loopback capture with endpoint mute, silence guard, and crash marker (`SPEC-25b`):**
   - When Linked-speaker mute (`muteLinkedWhileBridging`) is enabled, the Linked PC captures via WASAPI `VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK` (`PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE`) and mutes the default render endpoint (`IAudioEndpointVolume::SetMute(true)`), persisting prior mute state to `%LOCALAPPDATA%\AnyLeap\audio_mute_marker.json` for crash recovery and pausing emission during `AUDCLNT_BUFFERFLAGS_SILENT` frames.
4. **Drag & drop as Experimental (default off) with ≤ 2 s termination guarantee (`SPEC-25a`):**
   - Cross-PC drag & drop (`dragDrop`, default `false`) is gated behind an Experimental toggle because target apps and OLE capture ownership can reject synthesized drops (`DROPEFFECT_NONE`).
   - All drag staging and `DoDragDrop` lifecycles enforce a ≤ 2 s termination guarantee on cancellation, button release, or disconnect so neither PC can be left with a stuck modal drag loop or trapped input.

## Consequences
### Positive
- Large clipboard copies no longer saturate the LAN link speculatively; both peers can cancel in-flight transfers cleanly.
- Unreliable `0x41`-tagged datagrams keep audio latency low (~40 ms jitter buffer) without altering mouse datagram parsing.
- Process loopback + crash marker prevent audio feedback loops and recover speaker mute state after unexpected exits.
- Defaulting drag & drop to Experimental keeps `CF_HDROP` copy-paste as the reliable primary workflow while allowing opt-in testing.
### Negative
- Uncompressed 48 kHz stereo PCM consumes ~1.5 Mbps on the LAN while audio is active.
- Process loopback requires Windows 10 build 20348+ / Windows 11.
- Incoming `DragFile`/`ClipFile` stream headers are not yet strictly bound to active offer IDs and declared sizes (tracked in `docs/BACKLOG.md`).
