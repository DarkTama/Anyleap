# SPEC-19: KVM QUIC Transport & Authenticated Pairing

> **Status:** Approved (2026-11-13) | Target Version: `v1.0.1`
> Supersedes the WebSocket + raw UDP transport of SPEC-12. Decision record: ADR-0003.

## 1. Problem

The Computer Link (SPEC-12) transport has four defects, two of them critical:

| # | Severity | Defect | Location |
|---|----------|--------|----------|
| 1 | Critical | `/ws` accepts any LAN client. The PIN is checked only by `/pair/confirm`; the session token is generated but never stored or required. Any host on the network can connect and inject `KeyEvent`/`MouseButton` packets. | `kvm/server.rs` `ws_handler`, `confirm_handler` |
| 2 | Critical | `FileChunk.name` is joined onto the download dir unsanitised (`download_dir.join(name)`). A name like `..\..\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup\x.bat` writes anywhere the user can. | `kvm/commands.rs` `write_file_chunk` |
| 3 | High | All traffic (keystrokes, clipboard, files) is plaintext. UDP mouse deltas are unauthenticated. | `server.rs`, `client.rs` |
| 4 | Medium | Files and clipboard images share the single WS with input events (head-of-line blocking → keyboard lag during transfers) and are JSON-encoded `Vec<u8>` (~3–4× bloat). PIN derives from `SystemTime` micros (predictable). | `commands.rs` `kvm_send_file`, `server.rs:76` |

## 2. Goals

- One encrypted, mutually authenticated QUIC connection per peer pair.
- Pairing requires the 6-digit PIN once; afterwards peers authenticate by pinned certificate fingerprint.
- Input events never queue behind bulk data.
- Received file names can never escape the transfer directory.

Non-goals: internet/NAT traversal, relays, Android peers.

## 3. Design

### 3.1 Dependencies (`src-tauri/Cargo.toml`)
Add: `quinn = "0.11"`, `rustls = { version = "0.23", default-features = false, features = ["ring", "std"] }`, `rcgen = "0.13"`, `rand = "0.8"`, `sha2 = "0.10"`.
Remove once unused: `axum`, `tokio-tungstenite`, `futures-util` (verify no other module uses them).

### 3.2 Identity
- On first KVM start, generate a self-signed cert + key with `rcgen`; persist under the app data dir (`kvm_identity.der` / `kvm_identity.key`). Reuse thereafter.
- Fingerprint = SHA-256 of the cert DER, hex.
- Trust store: `kvm_trusted_peers.json` in the app data dir — `[{ computer_id, name, fingerprint }]`.

### 3.3 Endpoint
- Single QUIC endpoint on **UDP 4177** (server). TCP 4177 and UDP 4178 are retired. Discovery (UDP 4179) unchanged; beacons advertise the QUIC port.
- ALPN: `anyleap-kvm/1`.
- TLS: both sides present their cert (client auth required). Custom `rustls` verifiers accept any self-signed cert at the TLS layer; authorisation happens in 3.4 against the fingerprint.
- `keep_alive_interval = 2s`, `max_idle_timeout = 10s`. The QUIC idle timeout replaces `KvmPacket::Heartbeat`; connection loss emits `SessionDisconnect` exactly as today.
- Datagrams enabled.

### 3.4 Authorisation (server side, after handshake)
1. Peer fingerprint in trust store → session accepted.
2. Unknown fingerprint → the **only** permitted message is `Pair { computer_id, name, pin }` on the control stream.
   - PIN compared in constant time. On match: add peer to trust store, reply `PairAck { accepted: true }`, session accepted.
   - On mismatch: `PairAck { accepted: false }`, close. After 5 failures per server run, regenerate the PIN.
   - Any other message from an unknown peer closes the connection.
3. Client side: on successful pairing, store the server fingerprint; on later connects, reject a server whose fingerprint differs from the stored one (surface "peer identity changed — re-pair" to the UI).
- PIN generated with `rand::rngs::OsRng`, range 100000–999999.

### 3.5 Channels
| Traffic | QUIC primitive | Encoding |
|---------|----------------|----------|
| Control: Pair/PairAck, MouseButton, MouseWheel, KeyEvent, CursorEnter/Return/Position, ClipboardText, EmergencyRelease, SessionDisconnect | One bidirectional stream opened by the client | `u32` big-endian length prefix + `serde_json` of `KvmPacket` |
| Mouse delta | Unreliable datagram | Fixed 12 bytes LE: `dx: i32, dy: i32, seq: u32` |
| File | One unidirectional stream per file | Length-prefixed JSON header `{ kind: "file", name, size }`, then raw bytes to stream end |
| Clipboard image | One unidirectional stream per image | Header `{ kind: "clipboard_image", format }`, then raw bytes |

`KvmPacket::FileChunk`, `ClipboardImage`, `Handshake`, `HandshakeAck`, `Heartbeat` are removed from the enum; `UdpMouseDelta` becomes a datagram codec.

### 3.6 File safety
Receiver keeps only `Path::new(name).file_name()`; rejects empty, `.`/`..`, and Windows reserved names (`CON`, `NUL`, `COM1`…). On collision, append ` (n)` before the extension. Write to `<name>.part`, rename on completed stream; delete `.part` if the stream resets. Stream byte count must equal header `size`.

### 3.7 Frontend
Replace the HTTP `fetch` calls in `src/components/computers/ComputersView.tsx` and `PairingPinModal.tsx` with Tauri commands `kvm_pair_peer(ip, port, pin)` and `kvm_connect_peer(...)`. Add a "Forget peer" action that removes the trust-store entry.

## 4. Verification
Rust tests (`cargo test --manifest-path src-tauri/Cargo.toml`):
1. File-name sanitiser: traversal, absolute paths, reserved names, collisions.
2. Datagram codec and length-prefix framing round-trip.
3. Loopback integration: server + client on `127.0.0.1`:
   - correct PIN → paired, `KeyEvent` delivered;
   - wrong PIN → rejected, nothing delivered;
   - unpaired client sending `KeyEvent` → connection closed, nothing delivered;
   - paired client reconnects without PIN;
   - 5 MB file transfer arrives byte-identical while control messages still flow.
Plus the AGENTS.md gates: `npm test`, `cargo check`, `npm run build`.

Manual: two PCs on LAN — pair, edge-leap, type, clipboard text/image, file transfer; confirm Wireshark shows only QUIC (no plaintext).

## 5. Resolved questions
- Windows Firewall: rely on the first-run OS prompt; no installer rule in this spec.
- Migration: existing peers re-pair once; no migration code.
