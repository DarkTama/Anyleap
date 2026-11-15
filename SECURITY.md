# Security Policy

## Supported Versions

Security updates are provided for the **latest minor release** of AnyLeap:

| Version | Supported |
|---|---|
| `1.1.x` (latest) | Yes |
| `< 1.1` | No |

## Reporting a Vulnerability

**Please do not report security vulnerabilities through public GitHub issues or Discussions.**

Instead, use GitHub's private vulnerability reporting (**Security -> Advisories -> Report a vulnerability** on the repository):
- https://github.com/DarkTama/Anyleap/security/advisories/new

Please include:
- A clear description of the issue and potential impact.
- Step-by-step reproduction instructions or a minimal proof of concept.
- The AnyLeap version and OS you tested on.

## Scope

Reports affecting the following security boundaries are especially relevant:
- **Pairing & authentication:** 6-digit pairing code validation/rate-limiting, SHA-256 certificate fingerprint pinning (`kvm_trusted_peers.json`), and mutual TLS 1.3 / QUIC (`anyleap-kvm/1`) peer verification.
- **Input injection:** Unauthorized input injection or cursor-trap bypass in Shared Mouse & Keyboard (`kvm/injector.rs`, `kvm/server.rs`).
- **File transfer path handling:** Path traversal, symlink escapes, or size-validation bypasses in incoming file staging (`kvm/staging.rs`, `kvm/files.rs`) or phone file operations.
- **Open on Linked PC:** URI scheme filtering, executable safety guards, or argument-injection issues (`kvm/open_on_linked.rs`).
- **Auto-updater signing:** Minisign Ed25519 signature verification and update feed integrity.

## Response Expectations

AnyLeap is maintained by a solo maintainer on a best-effort basis. I aim to acknowledge private reports within a few days, investigate the root cause, and coordinate a fix before public disclosure.
