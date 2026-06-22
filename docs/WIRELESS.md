# Wireless & Connection Flows — AnyLeap *(working title)*

_Planning doc. Last updated: 2026-06-22._

This is the heart of the project: making the connection effortless. Everything here is driven by the **bundled `adb` CLI** — we implement **no networking crypto or mDNS responder ourselves**. This is the same approach proven by [adb-qr](https://github.com/aleixrodriala/adb-qr) and [escrcpy](https://github.com/viarotel-org/escrcpy).

## Background: Android 11+ wireless debugging

Android 11+ supports wireless ADB with two pairing methods, both built on mDNS service discovery:

- `_adb-tls-pairing._tcp` — the short-lived **pairing** service.
- `_adb-tls-connect._tcp` — the persistent **connect** service.

adb has its **own built-in mDNS stack**, so we can discover both by parsing `adb mdns services` — no Bonjour dependency, and it works even where the app process itself can't see LAN multicast (e.g. WSL2/VM), because adb does the discovery.

---

## Flow 1 — QR-code pairing (primary, the "magic")

This is the slick path, and — importantly — it does **not** require us to run a pairing server.

```
App                          Phone
 │  generate name+password
 │  show QR:                   user: Wireless debugging
 │  WIFI:T:ADB;S:<name>;       → "Pair device with QR code"
 │       P:<password>;;        → scans the QR
 │                             │
 │                             ├─ phone starts advertising
 │                             │  _adb-tls-pairing._tcp (name=<name>)
 │  poll `adb mdns services`   │
 │  ───────────────────────────┘  (find ip:port for <name>)
 │  adb pair ip:port <password>     ──────────────►  paired ✓
 │  poll `adb mdns services` for _adb-tls-connect._tcp
 │  adb connect ip:port             ──────────────►  connected ✓
 │  launch scrcpy                   ──────────────►  mirroring ✓
 │  save device for next time
```

**Why this works without custom crypto:** when the phone scans the QR, *the phone* becomes the mDNS advertiser and runs the pairing endpoint. We are the **client**: we just discover it via `adb mdns services` and call `adb pair` with the password we embedded in the QR. adb handles the SPAKE2/TLS handshake internally.

QR payload format (identical to Android Studio): `WIFI:T:ADB;S:<service-name>;P:<password>;;`

> Note: there is an *alternative* QR architecture (used by Android Studio's own UI) where the **desktop** advertises the service and runs the pairing server. We deliberately do **not** use that — it would require implementing the mDNS responder + SPAKE2 server ourselves. The adb-delegation model above avoids all of it.

## Flow 2 — Pairing-code pairing (fallback)

For when the camera/QR isn't convenient:

```
1. User: Wireless debugging → "Pair device with pairing code"
   Phone shows  ip:port  and a 6-digit code.
2. App auto-fills ip:port from `adb mdns services` (user types only the 6 digits).
3. adb pair ip:port <code>
4. Discover _adb-tls-connect._tcp → adb connect ip:port
5. Launch scrcpy → save device.
```

## Flow 3 — USB → wireless handoff (classic, for older phones / first setup)

For devices where wireless debugging pairing isn't available or as a one-time bootstrap:

```
1. Phone plugged in via USB, USB debugging authorized.
2. adb tcpip 5555           (restart adb in TCP mode on the device)
3. Read device IP (adb shell ip route / settings).
4. Unplug → adb connect <ip>:5555 → launch scrcpy → save device.
```

## Flow 4 — Auto-reconnect (the payoff)

On app launch, for each saved device:

```
1. Try `adb mdns services` → if the saved device's connect service appears, adb connect.
2. Else try the last-known host:port directly (adb connect).
3. On success, show it as "ready" with a one-click Mirror button.
```

After the first successful pair, **the user should never type anything again.**

## Discovery: parsing `adb mdns services`

Output looks roughly like:

```
List of discovered mdns services
<name>  _adb-tls-pairing._tcp  192.168.1.42:37123
<name>  _adb-tls-connect._tcp  192.168.1.42:42135
```

We poll on an interval while a relevant view is open, parse lines into `{name, service, host, port}`, and match against the QR's `<name>` (for pairing) or saved devices (for reconnect).

## Connection state machine (per device)

```
UNKNOWN ─discover→ FOUND ─pair→ PAIRED ─connect→ CONNECTED ─mirror→ MIRRORING
   ▲                                                  │              │
   └──────────────── disconnected / error ◄───────────┴──────────────┘
```

## Known gotchas (design around these)

- **AP isolation / "client isolation"** on some routers blocks device-to-device mDNS and connections. Detect failure and tell the user.
- **Stale saved IP** after the phone rejoins Wi-Fi or reboots → re-discover via mDNS rather than trusting last IP.
- **adb server version conflicts** if the user already runs a different adb → always invoke our pinned bundled adb.
- **Wireless debugging toggles off** on reboot on some OEMs (incl. Transsion) → reconnect may need the user to re-enable it; surface a clear hint.
- **Same-subnet requirement** — phone and PC must be on the same network/VLAN.

## References

- [adb-qr](https://github.com/aleixrodriala/adb-qr) — the adb-delegation QR model we copy (Python).
- [teamclouday/adb-wireless](https://github.com/teamclouday/adb-wireless) — terminal QR for adb.
- [escrcpy](https://github.com/viarotel-org/escrcpy) — production GUI doing QR pairing + LAN auto-discovery.
- [scrcpy#6509](https://github.com/genymobile/scrcpy/issues/6509) — upstream discussion on integrating adb QR codes.
- If we ever want a fully-native (no-adb-CLI) path: Rust crates [`spake2`](https://crates.io/crates/spake2), [`mdns-sd`](https://crates.io/crates/mdns-sd), [`adb_client`](https://crates.io/crates/adb_client). Not needed for the planned approach.
