# ADR 0003: QUIC Transport for Computer Link

## Status
Accepted

## Context
Computer Link (ADR-0002, SPEC-12) used a plaintext WebSocket for control/clipboard/files plus raw UDP for mouse deltas. The WebSocket accepted unauthenticated clients, traffic was unencrypted, and bulk transfers blocked input events on the same ordered channel.

## Decision
Use QUIC (`quinn` + `rustls`) as the single peer-to-peer transport:
- Self-signed per-machine certificates, authenticated by SHA-256 fingerprint pinned at PIN pairing (trust on first pair).
- One bidirectional stream for control/input, unreliable datagrams for mouse deltas, one unidirectional stream per file or clipboard image.
- LAN only; no relay or NAT traversal layer.

## Consequences
### Positive
- Encryption and mutual authentication without a custom crypto layer.
- Independent streams remove head-of-line blocking between input and bulk data.
- One UDP port instead of TCP + UDP; future PC display streaming (SPEC-13) can reuse the connection.
### Negative
- New dependencies (quinn, rustls, rcgen).
- Existing peers must re-pair once.
- UDP-only transport may need a firewall rule on some networks.
