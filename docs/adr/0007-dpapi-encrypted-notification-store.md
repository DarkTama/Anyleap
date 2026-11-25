# ADR 0007: Windows DPAPI Encrypted Local Store for Phone Hub Notifications and Transactions

## Status
Proposed (2026-11-25; accompanies SPEC-33)

## Context
SPEC-33 adds opt-in persistence for Phone Hub notifications (`Keep history` for 7/30/90 days) and parsed financial summaries (`Transactions only` or Money-source extractions), stored locally on the user's PC. Until now, Phone Hub held all notification and message state in memory only (`src/stores/phoneHubStore.ts`, `src-tauri/src/phone_hub/messages.rs`), persisting only non-sensitive preferences (`phoneHubMutedPackages`, `phoneHubNotificationPollSecs`) in `settings.json`.

Persisting notifications, transaction records, or user correction patterns in `settings.json` or plaintext files would expose one-time passcodes, private messages, merchant/counterparty patterns, and personal financial activity to other local OS users, offline disk inspection, and roaming profile/backup copies. At the same time, the store must remain simple, local-only (never synced over KVM or network), fast enough for a 10-second poll cadence, and free of user-managed master passwords that would block silent background capture.

## Decision
1. **Encrypt vaults with Windows DPAPI (`CryptProtectData` / `CryptUnprotectData`).**
   - Use the `windows` crate's `Win32_Security_Cryptography` API (`CRYPTPROTECT_UI_FORBIDDEN`, `dwFlags = 0` current-user scope) with an application-specific entropy tag (`b"AnyLeap.PhoneHub.Vault.v1"`).
   - Store two separate versioned encrypted files under `app_local_data_dir()` (`%LOCALAPPDATA%\<bundle-id>\`, never roaming `%APPDATA%`):
     - `phone_hub_history.v1.dpapi` (`HistoryVault` — raw notification history, `7` / `30` / `90` days)
     - `phone_hub_transactions.v1.dpapi` (`TransactionsVault` — parsed transactions, amount-less `LiabilityEvent` records, `sub_account_nicknames`, per-vault random 32-byte `rule_salt`, and user `CorrectionRule` entries)
2. **Independent retention clocks and `0..n` Money-source gating.**
   - Notification retention (`none` / `history` 7–90d / `transactions`) and `0..n` Money sources (`bank` / `ewallet` / `paylater` / `credit_card`, each paired with a rule template) are stored independently per app. Classification is per notification; unmatched alerts never enter `TransactionsVault`.
   - `TransactionsVault` follows its own `transactionRetention` setting (**`1 year` default / `2 years` / `Forever`**). Expiring a `StoredNotification` in `HistoryVault` after 7–90 days never deletes derived `StoredTransaction` or `LiabilityEvent` records in `TransactionsVault`, and age-based pruning never deletes `CorrectionRule` or `sub_account_nicknames` entries.
3. **Non-sensitive settings stay in `settings.json`.**
   - Opt-in flag (`historyEnabled`), per-app retention & `0..n` Money-source selections (`retention`, `retentionDays`, `moneySources`, `sensitive`), `transactionRetention`, `transferMatchWindowMinutes`, and `largeIncomeThresholdByCurrency` contain no notification payloads, names, card numbers, or amounts and remain in `settings.json` (`notificationStorage`).
4. **Atomic dirty-only writes and bounded capacity.**
   - Deduplicate incoming `(hw_serial, key, posted_unix)` tuples in memory before touching disk; re-encrypt and atomically write (`temp -> flush -> rename`) only when records are inserted, matched, manually corrected, pruned, or deleted.
   - Cap vaults (`5,000` history entries, `50,000` transactions) so plaintext payloads stay small (< 15 MB worst case) and DPAPI round-trips stay under ~15 ms.
5. **Transactions-only minimization, `sub_account` & salted-hash similarity rules.**
   - Card and account numbers (including masked card strings and raw last-4 digits) are **never** stored in `TransactionsVault`. Multiple cards/accounts on one source are distinguished solely via an opaque salted hash `sub_account = "sa_" + hex(SHA-256(rule_salt || "sub_account::" || package || "::" || source_id || "::" || last4))`, with optional user-assigned nicknames stored in `TransactionsVault.sub_account_nicknames`.
   - For apps configured as `Transactions only`, raw notification text, titles, sender names, and card/account numbers are parsed in memory and immediately discarded; only typed financial fields (`id`, `dedupe_id`, `hw_serial`, `package`, `source_id`, `app_label`, `source_type`, `sub_account`, `direction`, `amount_minor`, `scale`, `currency`, `category`, `classification`, `transfer_pair_id`, `linked_liability_event_id`, `transfer_hint`, `similarity_hash`, `statement_due_minor`, `statement_due_unix`, `timestamp_ms`, `rule_id`, plus optional manual override fields) and `LiabilityEvent` records are written to `TransactionsVault`.
   - `similarity_hash` is computed in memory as `SHA-256(rule_salt || package || "::" || direction || "::" || normalized_counterparty_or_va)` so `"Apply to similar in the future"` correction rules never store raw merchant names or virtual-account numbers on disk.
6. **Fail closed off-Windows.**
   - On non-Windows targets (`#[cfg(not(windows))]`), encrypted persistence reports an explicit unsupported-platform error and never falls back to writing plaintext files.

## Alternatives Considered
- **Plaintext JSON in `app_data_dir()` (like `phone_hub_apps.json`):** Zero implementation cost, but exposes OTPs, private messages, and spending history to any local user or backup reader. Rejected.
- **Embedded SQLite with cipher extension (or DPAPI-wrapped DEK + per-row AEAD):** Better scalability for hundreds of thousands of rows, but introduces native C build complexity or extra crypto dependencies (`aes-gcm` / `chacha20poly1305`) and schema migrations for data bounded to a few megabytes. Can be adopted later behind the same `NotificationVault` trait if scale demands it.
- **User-supplied master passphrase (Argon2 + AEAD):** Protects even against processes running as the same Windows user, but requires an unlock prompt on every app start, breaking unattended background capture. Rejected as default UX.

## Consequences

### Positive
- Zero new third-party cryptography crates; relies solely on the existing `windows` crate (`0.61`) and OS-managed per-user DPAPI keys.
- Automatic per-user key management with zero passphrase prompts.
- `%LOCALAPPDATA%` placement keeps vaults off Windows roaming profiles and separate from `settings.json`.
- Clean separation between `HistoryVault` and `TransactionsVault` lets raw notification history expire in 7–90 days while retaining 1y/2y/Forever transactions and salted-hash correction rules safely.

### Negative
- **Same-user process boundary:** Any process running under the same logged-in Windows user account can call `CryptUnprotectData` with the matching entropy. Documented in SPEC-33 §7.
- **Windows-bound portability:** Copying `.dpapi` files to another PC or Windows user SID makes them undecryptable (fails closed; user must delete and start fresh).
- **Windows-only persistence:** Non-Windows builds cannot persist notification history until a platform keychain adapter (e.g., OS Keyring + AEAD) is added if AnyLeap ships non-Windows builds.
