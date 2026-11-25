# AnyLeap Privacy Policy

> **Summary**
>
> **AnyLeap runs on your computers. It has no account, no cloud, and no tracking.**
>
> AnyLeap connects your Android phone and Windows PCs directly over USB or your own local network. Your screen, audio, files, clipboard, and notifications stay on your devices. The only internet requests AnyLeap makes on its own are optional update checks to GitHub.

*Last updated: 2026-11-25*

---

## 1. What data AnyLeap accesses and why

AnyLeap processes only the data needed for the features you use:

- **Phone screen, camera, and audio:** Streamed live from your Android phone over USB or local Wi-Fi when you start a mirroring, camera, or audio-only session so you can view and control your device on your PC.
- **Phone device info and status:** Model, brand, Android version, hardware serial, battery level, storage/RAM usage, Wi-Fi/cellular signal, screen state, and active call state, read over ADB to display the Devices and Phone Overview tabs and reconnect wirelessly.
- **Phone notifications and chat messages:** Read over ADB while the Phone tab or desktop notification pop-ups are active, so you can read alerts and grouped chat threads on your PC. By default they are kept in memory only and cleared when your phone disconnects or AnyLeap exits. If you turn on **Save notifications on this PC**, the apps you choose are stored encrypted on this PC (see section 5).
- **Phone photos, videos, files, and installed apps:** Listed and transferred over ADB only when you browse Gallery, Files, or Apps, or drag/drop files onto a mirror window. Installed app names are cached locally so notifications and the app launcher show readable labels.
- **Shared Mouse & Keyboard (PC-to-PC):** When you link two PCs, AnyLeap captures mouse movement, clicks, keyboard input, monitor layout, and (if enabled) clipboard text/images, files, audio, and lock/sleep events on one PC to forward them directly to your other paired PC.
- **Diagnostics logs (opt-in, off by default):** When you enable **Record a diagnostics log** in **Settings → Advanced**, AnyLeap records low-level input, focus, and transfer timing events to help troubleshoot issues. Window titles are never logged, regular typed keys are redacted to key categories (such as `letter` or `digit`), and filenames/paths are replaced with one-way short hashes.

---

## 2. Where data is stored and for how long

AnyLeap stores configuration, caches, and user-saved files locally on your PC:

| Data | Location on Windows | Retention / Cleanup |
|---|---|---|
| **App preferences, mode defaults, saved phones, nicknames, activity timeline** | `%APPDATA%\id.co.dak.anyleap\settings.json` | Kept until you edit/forget them or delete the data folder; activity timeline is capped at 200 entries. |
| **Installed phone app labels & pinned apps** | `%APPDATA%\id.co.dak.anyleap\phone_hub_apps.json` | Refreshed on app-list sync; kept until you delete the data folder. |
| **PC-to-PC pairing keys & trusted peer fingerprints** | `%APPDATA%\id.co.dak.anyleap\kvm_identity.der`, `kvm_identity.key`, `kvm_trusted_peers.json` | Kept until you click **Forget** in Computers or delete the data folder. |
| **Shared Mouse & Keyboard settings & update check cache** | `%LOCALAPPDATA%\id.co.dak.anyleap\EBWebView` (`localStorage`) | Kept until cleared or app data is removed; update metadata cache refreshes every 30 minutes. |
| **Gallery thumbnails, viewed photos, and opened media caches** | `%LOCALAPPDATA%\id.co.dak.anyleap\phone_hub_thumbs\`, `photos\`, `phone_hub_media\` | Automatically evicted (oldest first) at 100 MB (thumbnails), 500 MB (photos), and 500 MB (media). |
| **Opt-in diagnostics logs** | `%LOCALAPPDATA%\id.co.dak.anyleap\logs\kvm-debug.log` (and `.log.1`) | Rotates at 5 MB (max 2 files), expires automatically after **7 days**, and can be wiped anytime via **Delete logs**. |
| **Cross-PC transfer staging & temporary opened files** | `%LOCALAPPDATA%\AnyLeap\staging\` (and `_cursors\`) | Capped at 4,096 MB total; finished transfers/temp `open-*` files expire after **24 hours**; cancelled transfers delete immediately. |
| **Audio mute crash-recovery marker** | `%LOCALAPPDATA%\AnyLeap\audio_mute_marker.json` | Deleted automatically when audio bridging stops or on next startup. |
| **Screenshots, downloaded phone files, & large PC transfers** | `%USERPROFILE%\Downloads\AnyLeap\` (configurable) | Kept until you move or delete them. |
| **Screen recordings** | `%USERPROFILE%\Videos\AnyLeap\` | Kept until you move or delete them. |
| **Explorer "Open on Linked PC" menu entry (if enabled)** | `HKCU\Software\Classes\*\shell\AnyLeapOpenOnLinked` | Removed when you turn off the setting or uninstall AnyLeap. |
| **Phone notifications & chat conversations** | Memory (RAM) only, unless you opt in | Cleared on phone disconnect or app exit. |
| **Saved notification history (opt-in)** | `%LOCALAPPDATA%\id.co.dak.anyleap\phone_hub_history.v1.dpapi` (encrypted) | Per app: 7, 30, or 90 days, then removed automatically. Delete per app or all at once in **Settings → General**. |
| **Saved transactions (opt-in, planned)** | `%LOCALAPPDATA%\id.co.dak.anyleap\phone_hub_transactions.v1.dpapi` (encrypted) | Created empty today; filled once the transaction reader ships (see section 5). |

AnyLeap does **not** create or upload crash dump files, and it includes **zero analytics, telemetry, or tracking SDKs**.

---

## 3. What leaves your PC

- **To your own linked PC (LAN only):** When Shared Mouse & Keyboard is active, discovery beacons (computer ID, computer name, port, and screen dimensions) are broadcast on your local subnet over UDP port 4179. Once linked, all mouse/keyboard input, clipboard items, files, audio, and session signals travel directly over your local network via mutually authenticated **TLS 1.3 / QUIC** (UDP port 4177), verified by a one-time 6-digit pairing code and pinned SHA-256 certificate fingerprints. Nothing passes through external servers or relays.
- **To your own Android phone (USB or LAN only):** Commands, video/audio streams, and files travel directly between your PC and phone over USB or local Wi-Fi ADB.
- **Update checks (GitHub only):** If **Check for updates on launch** is enabled (or you click **Check now**), AnyLeap requests release metadata from `https://api.github.com/repos/DarkTama/Anyleap/releases` and signed installer manifests from `https://github.com/DarkTama/Anyleap/releases/latest/download/latest.json` (verified with Minisign Ed25519 signatures). Only standard HTTPS request headers and your IP address (as required for any HTTPS connection) reach GitHub; no device serials, usage metrics, or personal identifiers are sent. You can turn off automatic update checks in **Settings → General**.
- **Links you explicitly click:** Clicking **Open privacy policy** or a release note link opens `github.com` in your default web browser.

---

## 4. Opt-in features

Several features stay **off by default** until you turn them on:

- **Allow other PCs to control this PC** (Computers tab)
- **Record a diagnostics log** (Settings → Advanced)
- **Drag and drop files between PCs (Experimental)** (Settings → Clipboard & files)
- **Show phone notifications on this PC** (Settings → General)
- **Save notifications on this PC** and **Capture notifications while the Phone tab is closed** (Settings → General → Phone notifications & Money)
- **Ask where to save each time** (Settings → Clipboard & files)
- **Play Linked PC audio on this PC**, **Lock both PCs together**, and **Sleep both PCs together** (Settings → Audio, media & power)

You can also disable **Check for updates on launch**, **Share copied text**, **Share copied images**, **Copy and paste files between PCs**, and **Add "Open on Linked PC" to the right-click menu** at any time in **Settings**.

---

## 5. Opt-in notification storage (SPEC-33)

AnyLeap can keep a history of selected phone notifications on this PC. This is **off by default**.

- **Master switch:** Nothing is written to disk until you turn on **Save notifications on this PC** (Settings → General → Phone notifications & Money). Turning it off stops new writes; it does not silently erase what was already saved, so use **Delete all** if you want it gone.
- **Per-app choice:** Every app starts at **Don't keep** (memory only). You can switch an app to **Keep history** (7, 30, or 90 days) or **Transactions only**.
- **Encryption:** Saved data is encrypted with **Windows DPAPI** for your Windows user account and stored under `%LOCALAPPDATA%` (not the roaming profile). It is never sent to your linked PC or anywhere else. If the files cannot be decrypted (for example after copying them to another PC), AnyLeap refuses to read them and offers to delete and start fresh.
- **What DPAPI does not protect against:** other programs running under your own Windows account can ask Windows to decrypt the same data. Keep your account password private and your PC free of untrusted software.
- **Automatic cleanup:** History older than the app's chosen period is removed when AnyLeap starts and after new items are saved. Changing an app from **Keep history** to **Transactions only** removes its saved history; changing it to **Don't keep** asks whether to delete what was saved (selected by default).
- **Deleting:** Use the per-app **Delete** button or **Delete all** in **Settings → General**.
- **Capture while the Phone tab is closed:** Optional and off by default. When on, AnyLeap reads notifications whenever your phone is connected, so history is not limited to the time the Phone tab is open.
- **Local Hide:** Hiding a notification in AnyLeap only hides it in AnyLeap. It never dismisses anything on your phone.

> **Planned: Money (transactions).** A later release adds a reader for apps you mark as money sources. For apps set to **Transactions only**, it will keep only the direction (in or out), amount, currency, category, time, and app, never the notification text, names, or account and card numbers. Until that ships, **Transactions only** saves nothing.

---

## 6. How to delete everything

1. **Delete saved notifications in-app:** Open **Settings → General → Phone notifications & Money** and click **Delete all**.
1. **Delete diagnostics logs in-app:** Open **Settings → About → Privacy** (or **Settings → Advanced**) and click **Delete logs**.
2. **Forget paired devices and PCs:** Click **Forget** on saved phones in **Devices** or trusted PCs in **Computers**, and click **Clear** on the Activity Timeline if desired.
3. **Open and remove all app data:** Click **Open data folder** in **Settings → About → Privacy** (or uninstall AnyLeap first), then delete:
   - `%APPDATA%\id.co.dak.anyleap`
   - `%LOCALAPPDATA%\id.co.dak.anyleap`
   - `%LOCALAPPDATA%\AnyLeap`
4. **Remove saved user exports (if desired):** Delete `%USERPROFILE%\Downloads\AnyLeap` and `%USERPROFILE%\Videos\AnyLeap` (or your custom receive folder).

---

## 7. Children

AnyLeap is a general-purpose desktop utility and is not directed at children under 13 (or the minimum digital consent age in your jurisdiction). Because AnyLeap has no accounts, cloud servers, or tracking, it does not knowingly collect personal information from children.

---

## 8. Changes to this policy

If AnyLeap's data handling changes (for example, when planned features such as SPEC-33 ship), this `PRIVACY.md` file and the data inventory in [`docs/PRIVACY-AUDIT.md`](docs/PRIVACY-AUDIT.md) will be updated in the repository with a new *Last updated* date.

---

## 9. Legal note & principles

This policy is provided for transparency and **is not legal advice** for your own deployment or regulatory obligations. It is written to follow general data-protection principles—including Indonesia's Personal Data Protection Law (UU No. 27 Tahun 2022) and GDPR principles of purpose limitation, data minimisation, storage limitation, integrity/confidentiality, and user control—without claiming formal regulatory certification.

---

## 10. Contact

For privacy questions or requests, please open an issue in this repository's GitHub Issues:
- **Privacy & general questions:** <https://github.com/DarkTama/Anyleap/issues>
- **Security vulnerabilities (private reporting):** see [`SECURITY.md`](SECURITY.md)

For the engineering audit and exact `file:line` data inventory, see [`docs/PRIVACY-AUDIT.md`](docs/PRIVACY-AUDIT.md).
