# SPEC-33 Notification Tidy-Up, Per-App Retention, and Money Dashboard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Upgrade Phone Hub notifications with a tidy, privacy-aware UI, add an opt-in Windows DPAPI-encrypted local store with separate notification retention, Money-source configuration, and transaction retention (`1y` / `2y` / `Forever`), parse financial notifications strictly for user-marked Money sources with cross-account transfer matching, and deliver a `Phone → Money` dashboard with Spending vs Cash flow views, liability tracking, MoM/YoY comparisons, salted-hash correction rules, and CSV export.

**Architecture:**
- Phase 1 improves `NotificationsView.tsx` and adds a non-blocking lazy APK icon cache (`src-tauri/src/phone_hub/icons.rs`) with letter-avatar fallback, keeping the view strictly read-only.
- Phase 2 adds `src-tauri/src/phone_hub/vault.rs` (DPAPI `CryptProtectData` / `CryptUnprotectData` in `app_local_data_dir()`), deduplicating by `(hw_serial, key, posted_unix)`, keeping notification retention (`Don't keep` / `Keep history 7/30/90d` / `Transactions only`) independent of per-app `0..n` Money sources (`Bank` / `E-wallet` / `Paylater` / `Credit card` + rule template) and transaction retention (`1 year` / `2 years` / `Forever`), wired into **Settings → General** (`SPEC-28` single-home rule).
- Phase 3 adds `src-tauri/src/phone_hub/money.rs` (JSON rule pack, locale number parser, `0..n` source-scoped per-notification classification, transient salted-hash `similarity_hash` and opaque `sub_account` keys, amount-less `bill_paid` `LiabilityEvent` extraction, two-pass deterministic 1:1 transfer & bill-paid matcher, `Possible transfer` & `"Bill paid (amount unknown)"` flagging, plus an `#[ignore]` local real-capture aggregate test harness) and `src/components/phone/MoneyView.tsx` (`Phone → Money` basic dashboard).
- Phase 4 adds **Spending** *(default)* vs **Cash flow** views, **Paylater / Credit card** source + per-`sub_account` liabilities (`outstanding` + user nicknames + stated bill + **"Due next month"** vs monthly income), MoM/YoY per-category comparisons + **"Where did spending grow"**, manual corrections with `"Apply to similar in the future"` salted-hash rules plus `"Bill paid (amount unknown)"` linking, daily trends, category breakdowns, and formula-safe CSV export via SPEC-31 `transfer_paths`.

**Tech Stack:** Tauri 2 (Rust, `windows` 0.61 `Win32_Security_Cryptography`, `regex`, `sha2` / `rand`, `zip` or minimal APK entry reader, `serde_json`), React 19 + TypeScript + Tailwind (SPEC-26 Ink & Gold tokens), Zustand, Vitest, Cargo test.

**Specs & ADR:** `docs/specs/SPEC-33-notifications-and-money.md`, `docs/adr/0007-dpapi-encrypted-notification-store.md`.

## Global Constraints
- Follow `AGENTS.md` §1.3: **zero** third-party product, app, or bank names in code, rules, fixtures, comments, tests, or UI copy. Use generic terms (`bank app`, `e-wallet`, `paylater service`, `credit card`, `shopping app`) and fictional IDs (`com.example.bank`, `com.example.ewallet`, `com.example.paylater`, `com.example.creditcard`, `com.example.shop`).
- Follow `SPEC-28` §8.5 & §9.1: Settings → General is the single editor for persistence, per-app retention, Money sources, transaction retention, transfer match window, threshold, and delete controls; `NotificationsView` / `MoneyView` show read-only badges and link to Settings.
- Follow `SPEC-26` contrast & motion rules: `--text-muted` only on `--ink-900` / `--ink-950` (use `--text` on `--ink-800`); honor `prefers-reduced-motion`.
- Read-only Android invariant: never issue `adb shell cmd notification dismiss` or reply intents.
- Never log notification `title`, `text`, `bigText`, counterparty names, or transaction amounts (including in the local `#[ignore]` validation harness).
- Every phase ends green on all 5 `AGENTS.md` §2 gates (`npm test`, `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo check`, `npm run build`) and ships independently.

---

## Phase 1: Tidy Notification View & Lazy App Icons

### Task 1.1: Notification preview normalization, stacking, and date bucketing helpers (TDD)
**Files:** create `src/lib/notificationTidy.ts`, `src/lib/notificationTidy.test.ts`.
- [ ] Write failing Vitest tests in `src/lib/notificationTidy.test.ts`:
  - `collapseNotificationWhitespace`: collapses `\r\n`, tabs, and multiple blank lines into clean compact lines while preserving raw copy text.
  - `stackSameTitleNotifications`: clusters items sharing `(package, normalizedTitle)` within a group, placing newest first and returning `olderItems` for `+N more`.
  - `bucketByLocalDay`: partitions items into `Today`, `Yesterday`, and `Earlier` based on local midnight boundaries.
  - `formatRelativeNotificationTime`: formats `< 1m` (`Just now`), `< 1h` (`Xm ago`), same-day (`Xh ago`), yesterday (`Yesterday · HH:MM`), and older (`MMM D · HH:MM`).
  - `filterNotifications`: filters by selected `package` chip and case-insensitive search query across `appLabel`, `package`, `title`, and `text`.
- [ ] Implement `src/lib/notificationTidy.ts`; run `npx vitest run src/lib/notificationTidy.test.ts`.

### Task 1.2: Backend lazy app icon cache with letter-avatar fallback (TDD)
**Files:** create `src-tauri/src/phone_hub/icons.rs`; modify `src-tauri/src/phone_hub/mod.rs`, `src-tauri/src/lib.rs`, `src/services/phoneHubService.ts`.
- [ ] Write failing unit tests in `icons.rs` for cache path hashing, 48×48 PNG normalization, negative-cache TTL handling, and package validation (`validate_app_package`).
- [ ] Implement `IconCache` under `app_cache_dir()/phone_hub_icons/<hw_serial_hash>/<package>.png` (100 MB LRU cap via `phone_files::evict_dir_to_cap`), non-blocking `adb shell pm path <pkg>` + APK raster icon extraction (gracefully returning `Ok(None)` when unavailable), and Tauri command `phone_get_app_icon(serial, package)`.
- [ ] Register command in `src-tauri/src/lib.rs` and add `phoneGetAppIcon` wrapper in `src/services/phoneHubService.ts`.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml icons`.

### Task 1.3: Sensitive app masking, local Hide, and `NotificationsView` UI overhaul (TDD)
**Files:** modify `src/stores/phoneHubStore.ts`, `src/components/phone/NotificationsView.tsx`, `src/App.tsx`, `src/components/phone/__tests__/NotificationsAndMessages.test.tsx`.
- [ ] Add ephemeral `sensitivePackages` toggle (persisted preference in `settings.json` / `phoneHubStore`) and local `hiddenNotificationIds` state + actions (`hideNotificationLocal`, `unhideAllLocal`), plus toast preview masking in `src/App.tsx` when an app is marked sensitive.
- [ ] Update `NotificationsView.tsx`:
  - Top bar: search input (`role="search"`, Escape clears) + app filter chips (`All` + per-app chips with counts) + `"N hidden · Show"` chip when items are locally hidden.
  - Date sections (**Today**, **Yesterday**, **Earlier**) with collapsible app groups (`aria-expanded`), real app icon (`<img>` when cached, letter avatar fallback), read-only retention pill (`Live only · Settings`), per-app menu (`Open on PC`, `Mark/Unmark sensitive`, `Mute`).
  - Notification rows: 2-line clamp (`line-clamp-2`) + **"Show more"** / **"Show less"**, stacked `"+N more"` expander for same-app + same-title, local **Hide** button, and click-to-reveal placeholder for sensitive apps (auto-re-masks on unmount/tab switch).
- [ ] Expand `NotificationsAndMessages.test.tsx` to cover line clamp/Show more, title stacking, collapse, date sections, filter chips + search, sensitive masking/re-mask, local Hide, and letter avatar fallback.
- [ ] Run all 5 `AGENTS.md` §2 gates; commit Phase 1.

---

## Phase 2: Per-App Retention, Separate Money Source Config & DPAPI Store

### Task 2.1: Windows DPAPI vault module (`vault.rs`) (TDD)
**Files:** modify `src-tauri/Cargo.toml` (add `"Win32_Security_Cryptography"` feature to `windows`), create `src-tauri/src/phone_hub/vault.rs`, modify `src-tauri/src/phone_hub/mod.rs`.
- [ ] Write failing unit tests in `vault.rs`:
  - `#[cfg(windows)]` DPAPI `protect` / `unprotect` round-trip with entropy `b"AnyLeap.PhoneHub.Vault.v1"`; tampered ciphertext and mismatched entropy fail with an error.
  - `HistoryVault` & `TransactionsVault` atomic write/read (`temp -> flush -> rename`), capacity caps (`5,000` history / `50,000` transactions), `(hw_serial, key, posted_unix)` dedupe, and random 32-byte `rule_salt` initialization.
  - Independent pruning: `prune_expired_history` removes `7/30/90d` notifications without touching `TransactionsVault`; `prune_expired_transactions` honors `1y` (365d), `2y` (730d), and `forever` (no-op) while preserving all `CorrectionRule` entries.
  - `delete_for_package` and `delete_all` remove matching entries and rewrite/delete `.dpapi` files cleanly.
- [ ] Implement `vault.rs` in `app_local_data_dir()` (`phone_hub_history.v1.dpapi` and `phone_hub_transactions.v1.dpapi`) with `#[cfg(not(windows))]` failing closed (explicit unsupported error, zero plaintext writes).
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml vault`.

### Task 2.2: Retention poller integration & IPC commands (TDD)
**Files:** modify `src-tauri/src/phone_hub/poller.rs`, `src-tauri/src/phone_hub/mod.rs`, `src-tauri/src/lib.rs`, `src/services/phoneHubService.ts`.
- [ ] Add `NotificationStorageConfig` state to `PhoneHubState`, updated via `phone_set_notification_storage_config`.
- [ ] In `poll_notifications_once`:
  - When `history_enabled == false`, skip all vault writes (existing memory-only behavior).
  - When `history_enabled == true`: filter records by per-app `retention` (`none` skipped, `history` persisted to `HistoryVault` with `(hw_serial, key, posted_unix)` dedupe, `transactions` routed to Phase 3 parser in memory without ever storing raw text). Write disk **only** when dirty.
  - Trigger independent notification & transaction pruning on startup and dirty batches; handle `history` → `transactions` (purges notifications, retains transactions) and `→ none` transitions.
- [ ] Expose Tauri commands `phone_set_notification_storage_config`, `phone_get_notification_history`, `phone_set_notification_hidden`, `phone_delete_app_notification_data`, `phone_delete_all_notification_data`.
- [ ] Add poller/retention tests (confirming `"none"` writes nothing, `"transactions"` stores zero raw text, expiring `Keep history` notifications never deletes derived transactions, and idle duplicate polls cause zero disk writes); run `cargo test`.

### Task 2.3: Settings persistence & General UI (`N-01`..`N-08`) + History hydration (TDD)
**Files:** create `src/lib/notificationStorage.ts`, `src/lib/notificationStorage.test.ts`; modify `src/lib/settingsStore.ts`, `src/store/useAppStore.ts`, `src/components/settings/registry.ts`, `src/components/settings/GeneralSection.tsx`, `src/components/settings/__tests__/SettingsPanel.test.tsx`, `src/App.tsx`, `src/components/phone/NotificationsView.tsx`.
- [ ] Implement `NotificationStorageSettings` (`version: 2`, `historyEnabled: false`, `backgroundCapture: false`, `transactionRetention: "1y"`, `transferMatchWindowMinutes: 15`, `perApp: {}`) with separate `retention` and `moneySources: MoneySourceConfig[]` (`0..n` per app, each with `id`, `type`, `ruleTemplate`, `enabled`, optional `label`), normalization, load/save in `settingsStore.ts`, and startup sync to Rust.
- [ ] Add `N-01`..`N-08` to `REGISTRY` (`section: "general"`) and render the **"Phone notifications & Money"** group in `GeneralSection.tsx`:
  - `N-01` opt-in switch, `N-02` poll interval segmented control, `N-03` background capture switch, `N-04` per-app table (Retention policy + **Money sources (`0..n`)** list with **"Add money source"** picker for Type `Bank`/`E-wallet`/`Paylater`/`Credit card` & Rule template + per-source toggle/remove + `Sensitive` switch + per-app **Delete**), `N-05` Transaction retention (`1 year` default / `2 years` / `Forever`), `N-06` Transfer match window (`5 min` / `15 min` default / `30 min` or custom), `N-07` Large income threshold, `N-08` **Delete all** with confirmation modal.
- [ ] Wire `App.tsx` background watch (`["notifications"]`) when `historyEnabled && backgroundCapture` and a phone is connected.
- [ ] Hydrate saved history in `NotificationsView` when `historyEnabled` is `true` (including offline view for saved devices) and wire read-only policy + Money source badges linking to **Settings → General**.
- [ ] Add Vitest coverage for `notificationStorage.test.ts` (including `0..n` sources per app), `SettingsPanel.test.tsx` (`"Add money source"` Type + Rule template flow), `settingsFilter.test.ts`, and `NotificationsAndMessages.test.tsx`; run all 5 `AGENTS.md` §2 gates; commit Phase 2.

---

## Phase 3: `0..n` Sources, Per-Notification Parser, Two-Pass Matching & Basic Money

### Task 3.1: Data-driven JSON rule pack, per-notification `0..n` source scoping, locale parser, salted `sub_account` & `bill_paid` events (TDD)
**Files:** modify `src-tauri/Cargo.toml` (add `regex = "1"`, `sha2`, `rand` if not already present), create `src-tauri/resources/transaction_rules.default.json`, `src-tauri/src/phone_hub/money.rs`; modify `src-tauri/src/phone_hub/poller.rs`, `src-tauri/src/lib.rs`.
- [ ] Write failing Rust tests in `money.rs` using **fictional fixtures only**:
  - **Non-source gating:** pure shopping promo, email receipt, retail membership, and credit-score notifications with currency amounts produce **zero** transactions when `moneySources == []` (even when `retention == "history"`).
  - **Promo-from-money-app case (`0..n` sources & per-notification classification):** `com.example.shop` configured with `{ id: "shop-cc", type: "credit_card", ruleTemplate: "cobranded_card_in_shop_app" }` receives a promo (`"Flash Deal! Wireless Mouse Rp 245.500,00 today only"`) and a card purchase (`"Credit card transaction of Rp 245.500,00 at Sample Roastery on 25/11"`) → promo produces **0** records; purchase produces **1** `CreditCard` `Out` transaction (`amount_minor = 24550000`).
  - **Multiple source types on one app:** `com.example.shop` configured with both `paylater` and `credit_card` routes each notification strictly to its matching `(source_id, source_type)`.
  - **Two cards in one app (`sub_account` salted hash & zero raw digits):** two bill-payment alerts on `com.example.shop` (`4111-22**-****-0042` and `4111-22**-****-9012`) yield distinct opaque `sub_account` hashes (`sa_...`), emit amount-less `LiabilityEvent` (`BillPaid`, `UnmatchedNeedsReview`), and serialized `TransactionsVault` JSON contains **zero** occurrences of `"4111"`, `"0042"`, `"9012"`, or masked card strings.
  - `parse_localized_amount`: `1,021,403.00`, `1.021.403,00`, `45.000` (IDR), `120000`, `12.50`, `12,50`, negative/invalid inputs rejected.
  - English & Indonesian generic rules (`"You spent IDR 120,000.00 at Groceries."`, `"You received IDR 2,500,000.00 from Payroll at Income."`, `"Pembayaran Rp 45.000 di Makan"`, `"Transfer masuk Rp 1.021.403,00"`, `"Payment OUT Rp 245.500,00 — Card Bill"` with `TransferHint::Payment`, VA/top-up/bill hints, liability statement due patterns).
  - User rules file (`<app_local_data_dir>/transaction_rules.user.json`) overrides by `package` > `ruleTemplate` > `"*"` (without leaking `"*"` into strict mixed-app templates); malformed regex logs safe warning without crashing.
  - **Privacy & hash test:** verify `StoredTransaction` and `LiabilityEvent` contain zero raw notification text, sender names, or card/account numbers, and deterministic salted `similarity_hash` matches normalized counterparty/VA patterns.
- [ ] Implement `TransactionRulePack` loader, per-notification source-scoped evaluator, `parse_localized_amount`, `sub_account` salted hasher, and `LiabilityEvent` (`bill_paid`) extraction.

### Task 3.2: Two-pass cross-account transfer & amount-less `bill_paid` matcher + ignored local real-capture harness (TDD)
**Files:** create `src-tauri/src/phone_hub/transfer_matcher.rs` (or module in `money.rs`), modify `src-tauri/src/phone_hub/poller.rs`, `src-tauri/src/lib.rs`.
- [ ] Write failing Rust tests for all 12 matcher cases from SPEC-33 §12.1:
  - Pass 1 exact-amount `Transfer` (`bank` OUT + `ewallet` IN within ±15 min, same currency & amount), configurable window boundary, currency mismatch, distinct `(package, source_id)` check, greedy closest-time 1:1, deterministic tie-breaks (`liability_tie`, `in >= out`, `out.timestamp_ms`, `(out.id, in.id)`), and asset OUT + amounted liability IN (`DebtPayment` + `RepaymentReceived`).
  - **Bill paid with matching bank OUT:** bank posts generic `OUT` (`10:04`) and `Payment` `OUT` (`10:02`) within ±15 min of `bill_paid` (`10:05`) → prefers `Payment` / `VirtualAccount` / `Bill` line (`hint_tier = 0`), then closest in time 1:1; bank `OUT` becomes `DebtPayment` with its amount and `LiabilityEvent` becomes `Linked` reducing the card's outstanding balance.
  - **Bill paid with no match:** `bill_paid` with no asset `OUT` within `±W` → stays `UnmatchedNeedsReview` (`"Bill paid (amount unknown)"`) without altering balances.
  - **Bank OUT matching both transfer and bill (precedence):** bank `Payment` `OUT` (`Rp 500.000`, `12:00`), e-wallet `IN` (`Rp 500.000`, `12:02`), and `bill_paid` (`12:01`) → Pass 1 pairs `bank OUT ↔ ewallet IN` as `Transfer` (preventing phantom Income), while `bill_paid` falls back to any other eligible `OUT` or records `"Bill paid (amount unknown)"`.
  - Pass 3 fallback: unmatched `bank` OUT with `VirtualAccount`/`TopUp` hint → `PossibleTransfer`.
- [ ] Implement the two-pass matcher + `phone_get_transactions` returning `MoneyVaultSnapshot` (`transactions`, `liabilityEvents`, `subAccountNicknames`), wiring ingestion for both `"transactions"` and `"history"` apps with ≥ 1 enabled Money source in `poller.rs`.
- [ ] Add `#[ignore]` local real-capture validation harness test reading `ANYLEAP_REAL_CAPTURE_PATH` and printing **only aggregate counts** (per source type: `parsed_in`, `parsed_out`, `bill_paid_events`, `transfers_matched`, `bill_paid_linked`, `bill_paid_unknown`, `possible_transfers`, `unparsed_amount_lines`), never printing or writing text/amounts/names/card digits.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml money`.

### Task 3.3: `Phone → Money` sub-tab (basic summary, transfer exclusions, review flags, list) (TDD)
**Files:** create `src/lib/moneySummary.ts`, `src/lib/moneySummary.test.ts`, `src/components/phone/MoneyView.tsx`, `src/components/phone/__tests__/MoneyView.test.tsx`; modify `src/components/phone/PhoneView.tsx`, `src/types/phone.ts`, `src/services/phoneHubService.ts`, `src/App.tsx`.
- [ ] Write failing Vitest tests in `moneySummary.test.ts` for Today / This week / This month In/Out/Net using integer minor units (`amountMinor`), excluding `transfer`, `repayment_received`, and `ignored` rows from totals, multi-currency separation, `possible_transfer` + `"Bill paid (amount unknown)"` review counts, and large-income threshold flagging.
- [ ] Add `"money"` sub-tab to `PhoneView.tsx` immediately after `"notifications"`, with opt-in / source-setup CTA when `historyEnabled === false` or zero apps have an enabled Money source, linking to **Settings → General**.
- [ ] Implement `MoneyView.tsx` (basic) with:
  - Always-visible disclaimer banner (`"Estimate only — calculated from captured phone notifications while connected..."`).
  - Currency switcher (when > 1 currency), source filter, period/direction/review filters, Today / Week / Month summary cards, large income highlight badges (`--gold-400`), `"N items to review"` banner (covering `Possible transfer` + `"Bill paid (amount unknown)"`), and feed showing `Transfer between your accounts` / `Possible transfer` / `Debt payment` / `Bill paid (amount unknown)` badges plus non-identifying sub-account tags (`Card #1` or user nickname).
- [ ] Add `MoneyView.test.tsx` and run all 5 `AGENTS.md` §2 gates; commit Phase 3.

---

## Phase 4: Liabilities, Sub-Account Nicknames, Views, Comparisons, Corrections & CSV Export

### Task 4.1: Spending vs Cash flow views, Liabilities (per-source & per-`sub_account`) & "Due next month" (TDD)
**Files:** modify `src/lib/moneySummary.ts`, `src/lib/moneySummary.test.ts`, `src/components/phone/MoneyView.tsx`, `src/components/phone/__tests__/MoneyView.test.tsx`.
- [ ] Implement view-aware aggregation in `moneySummary.ts`:
  - **Spending** *(default)*: includes `paylater` / `credit_card` purchases at purchase time; excludes `debt_payment`, `transfer`, `repayment_received`, and `ignored`.
  - **Cash flow**: includes liquid (`bank` / `ewallet`) In/Out including `debt_payment` (whether matched via amounted `RepaymentReceived` or linked `bill_paid` `LiabilityEvent`); excludes `paylater` / `credit_card` purchases, `transfer`, and `ignored`.
  - **Liability balances & "Due next month":** compute per-source `(package, source_id)` and per-`sub_account` `purchases - repayments` (counting each `RepaymentReceived` and linked `bill_paid` `resolved_amount_minor` **once**), handle mixed `Some(sub_account)` + `None` (`"Unassigned"` bucket), pick up stated `statementDueMinor` / `statementDueUnix`, and compare **Total income this month** against **Liabilities due**.
- [ ] Add **Spending** / **Cash flow** toggle, per-liability & per-sub-account cards (supporting two cards in one app, showing nicknames or `"Card #1"`/`"Card #2"`, plus `"Bill paid (amount unknown)"` badge), and **"Due next month"** vs income panel in `MoneyView.tsx`, with full Vitest coverage.

### Task 4.2: Comparisons (MoM / YoY / "Where did spending grow"), Daily trends & Category breakdown (TDD)
**Files:** modify `src/lib/moneySummary.ts`, `src/lib/moneySummary.test.ts`, `src/components/phone/MoneyView.tsx`, `src/components/phone/__tests__/MoneyView.test.tsx`.
- [ ] Add pure aggregation helpers (`computeDailyTrends`, `computeCategoryBreakdown`, `computeCategoryComparisons`, `computeTopSpendingGrowth`) and tests in `moneySummary.test.ts` (including graceful handling when `< 13` months exist for YoY).
- [ ] Render **"Where did spending grow"** callout, MoM / YoY per-category delta table, flat Ink & Gold daily In/Out bar chart (7 d / 30 d / This month) with keyboard-accessible bar labels and screen-reader table fallback, and ranked category breakdown in `MoneyView.tsx`.
- [ ] Verify contrast (`--text` on `--ink-800`, `--text-muted` only on `--ink-900`/`--ink-950`) and `prefers-reduced-motion`.

### Task 4.3: Manual corrections with salted-hash memory, sub-account nicknames & liability event resolution (TDD)
**Files:** modify `src-tauri/src/phone_hub/vault.rs`, `src-tauri/src/phone_hub/money.rs`, `src-tauri/src/lib.rs`, `src/services/phoneHubService.ts`, `src/components/phone/MoneyView.tsx`, `src/components/phone/__tests__/MoneyView.test.tsx`.
- [ ] Implement `phone_apply_transaction_correction(tx_id, classification?, category?, apply_to_similar)` in Rust:
  - Updates the target transaction's `manual_classification` / `manual_category`.
  - When `apply_to_similar` is true and `similarity_hash` is present, upserts a `CorrectionRule` keyed by `(package, direction, similarity_hash)`, retroactively updates matching historical rows, and applies on all future ingestion (surviving transaction age pruning).
- [ ] Implement `phone_set_sub_account_nickname(sub_account, nickname?)` and `phone_resolve_liability_event(event_id, linked_tx_id?, ignore?)` in Rust (`TransactionsVault.sub_account_nicknames` survives age pruning; manual `bill_paid` link sets `manual_locked = true` and promotes the target `OUT` to `DebtPayment`).
- [ ] Add row correction popover (`Mark as transfer`, `Mark as debt payment`, `Ignore`, category edit, `Restore automatic`, `"Apply to similar in the future"`), inline sub-account nickname editor on liability/sub-account cards, and `"Link to outflow"` / `"Dismiss"` actions on `"Bill paid (amount unknown)"` rows in `MoneyView.tsx` + Vitest tests.

### Task 4.4: Formula-safe CSV export via SPEC-31 `transfer_paths` + Docs Sync (TDD)
**Files:** modify `src-tauri/src/phone_hub/money.rs`, `src-tauri/src/lib.rs`, `src/services/phoneHubService.ts`, `src/components/phone/MoneyView.tsx`, `src/components/phone/__tests__/MoneyView.test.tsx`, `docs/specs/SPEC-33-notifications-and-money.md`, `docs/adr/0007-dpapi-encrypted-notification-store.md`, `README.md`, `docs/MANUAL-TEST-CHECKLIST.md`.
- [ ] Write failing Rust test for CSV serialization (`timestamp_iso,app_id,app_label,source_id,source_type,sub_account,direction,classification,amount,currency,category`, never exporting raw card digits), formula-injection escaping (`=`, `+`, `-`, `@`, `\t`, `\r` prefixed with `'`), `transfer_paths::prepare_receive_dir()` / `unique_dest_path`, and `askWhereToSave` explicit path support.
- [ ] Implement `phone_export_transactions_csv` returning `PullResult { saved_path, fallback_from }`.
- [ ] Wire **Export CSV** button in `MoneyView.tsx` (respecting `useAppStore.transferFolders.askWhereToSave` via `saveDialog`, showing unencrypted-export note, and rendering **"Show in folder"** calling `revealItemInDir(savedPath)`).
- [ ] Synchronize docs per `AGENTS.md` §1.4 (`README.md`, `SPEC-33` status, `ADR-0007` status, `MANUAL-TEST-CHECKLIST.md`) and run a third-party-name grep check.
- [ ] Run all 5 `AGENTS.md` §2 gates (`npm test`, `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo check`, `npm run build`); commit Phase 4.

---

## Risks & Mitigations

1. **Mixed-purpose app rule bleed (promotions counted as spending):** A shopping app with a co-branded credit card posts promotional prices alongside real card alerts. *Mitigation:* bind each `MoneySourceConfig` to a strict `ruleTemplate` (`cobranded_card_in_shop_app` omits generic `"*"` fallback rules) so unmatched promo notifications always produce zero records.
2. **Asymmetric card digits across notifications:** Purchase notifications may omit last-4 digits (`sub_account = None`) while bill-paid confirmations include masked digits (`sub_account = Some(key)`). *Mitigation:* aggregate liabilities at both the source level `(package, source_id)` and per-`sub_account` level (with an `"Unassigned"` bucket when needed), and never store raw card/account digits.
3. **Amount-less `bill_paid` false matching vs transfer matching:** An amount-less `bill_paid` alert has no amount constraint and could steal an `OUT` belonging to a bank→e-wallet transfer or match an unrelated purchase within ±15 min. *Mitigation:* run Pass 1 exact-amount matching before Pass 2 amount-less linking, prioritize `Payment`/`VirtualAccount`/`Bill` outflows in Pass 2, leave unmatched bills as `"Bill paid (amount unknown)"`, and allow manual link/unlink in `MoneyView`.
4. **Vault schema evolution across phases:** Phase 2 initializes `TransactionsVault` before Phase 3/4 populate `liability_events` and `sub_account_nicknames`. *Mitigation:* use `#[serde(default)]` on new `TransactionsVault` and `StoredTransaction` fields (`source_id`, `sub_account`, `linked_liability_event_id`) so Phase 2 vaults deserialize cleanly without data loss.
5. **Pre-tracking starting balances & YoY retention window:** Carried-over credit card debt or the default `1 year` transaction retention can skew calculated balances or YoY comparisons. *Mitigation:* prefer stated `statementDueMinor` when available, display `"Calculated since tracking started"` when net balance floors at 0, and surface the `"Switch to 2 years or Forever"` hint on YoY.
6. **APK icon & Android summary truncation:** Split APKs/adaptive-only icons or bundled Android summary notifications (`"3 new transactions"`) may omit icons or individual lines. *Mitigation:* negative-cache icon misses and fall back to letter avatars; display the always-visible Money estimate disclaimer.
