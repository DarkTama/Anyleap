# SPEC-33: Notification Tidy-Up, Per-App Retention, and Money Dashboard

> **Status:** Phases 1–3 Implemented (2026-11-26); Phase 4 Draft
> **Scope:** Phone Hub notifications (`src/components/phone/NotificationsView.tsx`, `src/components/phone/MoneyView.tsx`, `src/components/phone/PhoneView.tsx`, `src/stores/phoneHubStore.ts`), backend notification store, DPAPI vault, transaction parser & transfer matcher (`src-tauri/src/phone_files.rs`, `src-tauri/src/phone_hub/`), and Settings (`src/components/settings/`).
> **Related:** SPEC-21 (Phone Files & Notifications), SPEC-26 (Ink & Gold Theme), SPEC-27 (Phone Hub), SPEC-28 (Settings Redesign), SPEC-31 (Default Transfer Folders), ADR-0007 (DPAPI Encrypted Notification Store).
> **Plan:** `docs/superpowers/plans/2026-11-25-spec33-notifications-money.md`

---

## 1. Problem & Evidence

Phone Hub currently treats Android notifications as a raw, transient feed:

1. **Cluttered, hard-to-scan list.** `NotificationsView` (`src/components/phone/NotificationsView.tsx:35-514`) renders full `whitespace-pre-wrap` text (`:342`) with blank lines intact, never stacks repeated alerts from the same app and title, cannot collapse app groups, offers no Today/Yesterday/Earlier sections, no app filter chips, no search box, and shows only single-letter initials (`:242,251-253`) instead of real app icons.
2. **No privacy controls on screen.** Sensitive apps (bank apps, e-wallets, authenticator apps, email apps) display their full notification body immediately in the list (`:332-345`) and toast preview (`:494-499`), with no click-to-reveal masking and no local per-notification "Hide" action.
3. **Zero persistence and lost financial alerts.** Neither the backend (`src-tauri/src/phone_hub/messages.rs:31-33,184-190`) nor the frontend (`src/stores/phoneHubStore.ts:35,88-90`) persists notification data to disk. Disconnecting the phone or dismissing a notification on Android erases transaction notifications.
4. **No safe middle ground between "keep nothing" and "keep raw prose."** Users who want spending/income tracking from bank, e-wallet, paylater, or credit card notifications should not have to persist raw notification text, sender names, or account numbers on disk, nor manually copy amounts into a spreadsheet.

### 1.1 Empirical Evidence from Real Notification Snapshot (Aggregate Counts Only)

In a 47-notification snapshot captured from a real device, currency amounts appeared across **7 apps**:
- A **bank app** (7 of 8 notifications)
- An **email app** (4 of 6 notifications — transaction receipts)
- An **e-wallet** (2 of 9 notifications)
- A **credit-score app**
- A **retail membership app**
- **2 shopping apps** (promotions or product prices, **not** completed transactions)

**Key design consequence:** A currency amount in notification text **must never** count as money in or out on its own. Furthermore, **an app is not limited to a single source type**: a mixed-purpose app (for example, a shopping app that also issues a co-branded credit card or paylater account) posts both promotional/price notifications and real financial or bill-payment notifications from the same Android `package`. Therefore, an app maps to **`0..n` Money sources** (each with its own type and rule set), and **classification is per notification** — notifications from a Money-source app that match no source rule (such as promotions with prices) never count.

---

## 2. Goals and Non-Goals

### Goals
- **G1. Tidy, scannable notification view (Phase 1).** Collapse excessive blank lines/whitespace, clamp bodies to 2 lines with "Show more", stack same-app + same-title items, allow collapsible app groups, group by Today / Yesterday / Earlier with relative times, provide app filter chips and search, display deterministic per-package colored letter avatars (real APK app icons deferred to a future task), mask sensitive apps until clicked, and support local-only "Hide".
- **G2. Opt-in, DPAPI-encrypted local persistence with separate notification and transaction retention (Phase 2).** Persistence is off until explicitly enabled by the user. Notification retention defaults to **Don't keep** per app (`Keep history` 7/30/90 days or `Transactions only`). Separately, each app configures **`0..n` Money sources** (`Bank` / `E-wallet` / `Paylater` / `Credit card`, each with a rule template), and parsed transactions follow a dedicated **Transaction retention** setting (`1 year` default / `2 years` / `Forever`) independent of notification expiration.
- **G3. Data-driven per-notification parser, salted sub-account keys & two-pass transfer/bill matching (Phase 3).** Parse financial notifications via a JSON rule pack (regex templates + locale number formatting for `1,021,403.00` and `1.021.403,00`) scoped strictly to each app's enabled Money sources (`0..n`), ignoring unmatched notifications (e.g. promos). Distinguish multiple cards/accounts via a salted-hash `sub_account` key (never storing card/account numbers). Run deterministic two-pass matching within a configurable ±15-minute window: exact-amount cross-source **Transfer** / **Debt payment** first, then amount-less **`bill_paid`** liability event linking (preferring `payment` / `virtual_account` / `bill` outflows, or flagging **`"Bill paid (amount unknown)"`** for review), and flag remaining unmatched bank VA/top-up outflows as **Possible transfer**.
- **G4. Liabilities, Spending vs Cash flow views, comparisons, corrections & export (Phase 4).** Support Paylater / Credit card expense-at-purchase-time accounting, Debt payment exclusions from Spending (shown in Cash flow), per-liability and per-sub-account outstanding balance (with user nicknames and **"Due next month"** vs monthly income), resolution for **"Bill paid (amount unknown)"**, MoM/YoY category comparisons + **"Where did spending grow"**, single-transaction manual corrections with salted-hash memory rules, daily trends, and formula-safe CSV export.
- **G5. Compliance.** Zero third-party product, app, or bank names (`AGENTS.md` §1.3); SPEC-28 single-home settings compliance; SPEC-26 Ink & Gold contrast and reduced-motion accessibility.

### Non-Goals
- **No phone-side dismiss or reply.** The notification view remains strictly read-only; AnyLeap never runs `cmd notification dismiss` or replies on Android.
- **No cloud sync or KVM sharing.** Encrypted notification history, transactions, and correction rules never leave the local PC or travel over the multi-PC KVM link.
- **No user rule-editor UI in Phase 3.** The JSON rule format supports user-supplied rules on disk, and Phase 4 adds inline transaction corrections with `"Apply to similar in the future"`, while a full regex builder UI is deferred.
- **No email receipt ingestion by default.** Email apps never feed Money by default; optional email receipt parsing may be offered in a future extension if the user explicitly opts an email app in.
- **No bank API integration, SMS scraping, or OCR.** Transaction estimates come strictly from Android notification records captured while connected.
- **No plaintext fallback.** On non-Windows targets where Windows DPAPI is unavailable, encrypted persistence refuses to write plaintext to disk.

---

## 3. Research: Current Notification Pipeline Today

All claims below were verified directly in the repository:

| Topic | Verified Behaviour | Source |
|---|---|---|
| **Parser entry point** | `parse_dumpsys_notifications_rich(output: &str) -> Vec<ParsedNotificationRecord>` parses `adb shell dumpsys notification --noredact` output and sorts newest-first (`Reverse(posted_unix)`). Filters out `FLAG_ONGOING_EVENT` (`0x2`) and `FLAG_GROUP_SUMMARY` (`0x200`). | `src-tauri/src/phone_files.rs:540-876`, `:618`, `:874` |
| **Extracted fields** | Parses `package` (`pkg=`), `key` (`key=`), `app_label` (`appName=`), `title` (`android.title`), `text`/`bigText`/`subText` (collapsed into one `base.text` preferring `bigText` > `text` > `subText`), `posted_unix` (seconds from `postTime=`, fallback `when=` / `mCreationTimeMs=`), `category`, `shortcut`, `flags`, `android.isGroupConversation`, and `android.messages` bundles. **Not emitted today:** Android notification `group=` / `groupKey=` and `channel=`, and `bigText` vs short `text` are merged. | `src-tauri/src/phone_files.rs:34-41`, `:375-386`, `:424-431`, `:624-630`, `:690-711`, `:780-795`, `:851-867` |
| **Backend poller & cadence** | `poll_notifications_once` runs `dumpsys notification --noredact`, resolves app labels via `AppLabelCache`, feeds `MessageStore::ingest`, and emits Tauri event `phone-notifications` (`PhoneNotificationsEvent { serial, items }`). Scheduled in `run_poller_loop` via `PollTier::Notifications(notif_secs)` with `DEFAULT_NOTIF_POLL_SECS = 10` and `MIN_NOTIF_POLL_SECS = 5`. | `src-tauri/src/phone_hub/poller.rs:20-21`, `:42-56`, `:275`, `:334-400`, `:446-451` |
| **When polling runs** | Polling runs only while `"notifications"` is watched: when `PhoneView` is mounted (`phoneHubWatch(activeSerial, ["overview", "notifications"])`) or when `activeTab !== "phone"` and `appPrefs.desktopNotifications` is `true` (`phoneHubWatch(activePhoneSerial, ["notifications"])`). When the Phone tab is closed and desktop toasts are off, polling stops. | `src/components/phone/PhoneView.tsx:252`, `src/App.tsx:245-251` |
| **On-demand refresh** | `get_device_notifications` wraps `parse_dumpsys_notifications` and resolves labels; called on manual Refresh in `NotificationsView`. | `src-tauri/src/phone_files.rs:408-413`, `:1103-1153`; `src/App.tsx:150-176` |
| **Frontend grouping & mute** | `NotificationsView` filters out `mutedPackages`, sorts newest-first, and groups strictly by `package`. Renders a single-letter avatar (`(group.label[0] \|\| "?").toUpperCase()`), full unwrapped text, Copy button, and per-app `Mute…` menu (`Open <label> on PC` / `Mute <label>`). Right sidebar holds Muted apps and Desktop toasts preview. | `src/components/phone/NotificationsView.tsx:73-97`, `:242`, `:251-253`, `:271-366`, `:374-509` |
| **App icon support** | None today. `AppLabelCache` (`phone_hub_apps.json`) caches only `PhoneApp { package, label, system }` from `scrcpy --list-apps` and pinned package IDs, keyed by `hw_serial`. | `src-tauri/src/phone_hub/apps.rs:38-42`, `:134-196` |
| **Persistence today** | **Zero notification payloads are persisted.** `usePhoneHubStore.notificationsBySerial` and Rust `MessageStore` are memory-only and cleared on disconnect or app exit. Only settings (`phoneHubMutedPackages`, `phoneHubNotificationPollSecs`, `appPrefs.desktopNotifications`) are stored in `settings.json`. | `src/stores/phoneHubStore.ts:9-10,35,84-100,181-199`; `src-tauri/src/phone_hub/messages.rs:31-33,184-190`; `src-tauri/src/phone_hub/mod.rs:73-75`; `src/lib/settingsStore.ts:30` |

---

## 4. Architecture & Placement Decisions

### 4.1 Money Dashboard Placement (`Phone → Money`, SPEC-28 Single-Home Rule)
**Decision:** Add **Money** as a new sub-tab inside the **Phone** workspace (`PhoneSubTab = "overview" | "messages" | "notifications" | "money" | "gallery" | "files" | "apps"`), placed immediately below **Notifications** in `PhoneView`'s sidebar (`src/components/phone/PhoneView.tsx:62-73`).

**Justification via SPEC-28 §9.1 (Single-Home Rule):**
- `Phone` is the single workspace for all ADB-derived phone views (`Overview`, `Messages`, `Notifications`, `Gallery`, `Files`, `Apps`) and owns the single device selector (`#phone-device-select`).
- Notification-derived features (`Notifications`, `Messages`, and `Money`) share the same device identity (`hw_serial`), app label/icon cache, and per-app configuration.
- Making `Money` a separate top-level tab alongside `Devices | Phone | Computers | Settings` would duplicate device selection and split notification-derived data across two top-level navigation items.
- **Note on disconnected state:** Unlike live-only views (`Overview`, `Gallery`, `Files`), stored history and Money summaries exist locally even when the phone is unplugged. `PhoneView` (`PhoneView.tsx:295-305`) should allow viewing `Money` (and saved `Notifications` history) for any known saved device (`hw_serial`) when no live phone is connected if local persistence is opted in, while keeping the live polling indicator clearly marked as `"Offline — showing saved history"`.

### 4.2 Settings Placement (SPEC-28 §9.1 Single-Home Rule)
**Decision:** All configuration for notification storage, per-app retention policies, Money source designation/types, transaction retention, transfer matching window, background capture, poll cadence, large-income threshold, and deletion lives in **Settings → General** under a **"Phone notifications & Money"** subsection (below `M-27` / `M-28`), preserving SPEC-28's 7-section left rail (`src/components/settings/registry.ts:15-23`).

- **Single editor:** Settings is the sole editor for opt-in (`N-01`), poll interval (`N-02`), background capture (`N-03`), per-app retention & `0..n` Money sources (`N-04`), transaction retention (`N-05`), transfer match window (`N-06`), large-income threshold (`N-07`), and bulk/per-app deletion (`N-08`).
- **Read-only links elsewhere:** `NotificationsView` displays each app's current retention badge (`Live only`, `History · 30d`, `Transactions only`) and optional Money source badges (`Bank`, `E-wallet`, `Paylater`, `Credit card`) as read-only pills linking to **Settings → General**, and `MoneyView` displays a link to **Settings → General** for source setup, retention, transfer window, and threshold changes (matching SPEC-28 §9.1 and `ReadOnlyLinkRow`).
- **In-context triage stays in their views:** Ephemeral click-to-reveal on a sensitive notification, local per-item **Hide**, and **Mute** stay in `NotificationsView`; single-transaction reclassifications (`Mark as transfer`, `Mark as debt payment`, `Ignore`, category override, `Apply to similar in the future`), linking/resolving **"Bill paid (amount unknown)"** events, and setting a **sub-account nickname** act as in-context data annotations inside `MoneyView`.

| ID | Section | Label | Helper text | Control | Keywords |
|---|---|---|---|---|---|
| `N-01` | `general` | Save notification history & transactions on this PC | `Off by default. Stored locally and encrypted for your Windows account; never synced.` | Switch (`role="switch"`, default `false`) | `["notification", "history", "persist", "store", "privacy", "money"]` |
| `N-02` | `general` | Notification check interval | `How often AnyLeap reads notifications while the phone is connected.` | Segmented: `5 s` / `10 s` / `30 s` (maps to `phoneHubNotificationPollSecs`) | `["poll", "interval", "refresh", "notification"]` |
| `N-03` | `general` | Capture notifications while Phone tab is closed | `Keeps watching for saved history and Money alerts whenever a phone is connected.` | Switch (disabled when `N-01` is off, default `false`) | `["background", "capture", "watch", "money"]` |
| `N-04` | `general` | App retention, Money sources & privacy | `Retention and Money sources are independent. Pick which notifications to keep, and add Money sources to track spending without counting unrelated alerts.` | **Search apps** filter (`Esc` clears, `Enter` never adds); configured apps first, unconfigured collapsed under **Other apps (N)**; per-app row: Retention (`Don't keep` / `Keep history` + `7d`/`30d`/`90d` / `Transactions only`), `Sensitive` switch, **Delete saved data** button, **Remove from list** (for manually added/invalid rows, resetting policy and optionally deleting saved data), inline warning + **"Use Transactions only"** fix when enabled Money sources exist under `Don't keep`, plus a **Money sources (`0..n`)** list (expanded on configured rows; compact **"Add money source"** action on unconfigured rows) with multi-type checkboxes (`Bank` / `E-wallet` / `Paylater` / `Credit card`, each using its generic template); secondary **"Add an app that isn't listed"** disclosure with Android package ID validation | `["retention", "policy", "app", "sensitive", "transactions", "money", "source", "bank", "ewallet", "paylater", "credit card", "template", "add money source"]` |
| `N-05` | `general` | Transaction retention | `How long parsed Money transactions are kept, separate from notification history.` | Segmented: `1 year` *(default)* / `2 years` / `Forever` | `["transaction", "retention", "years", "forever", "money", "expiry"]` |
| `N-06` | `general` | Transfer match window | `Max time difference to match an Out and In of the same amount across your accounts.` | Segmented / numeric input: `5 min` / `15 min` *(default)* / `30 min` (or custom minutes `1–60`) | `["transfer", "window", "minutes", "match", "money"]` |
| `N-07` | `general` | Large income highlight | `Highlight incoming transactions at or above this amount in Money.` | Number input + currency selector (default `0` = off) | `["money", "income", "threshold", "highlight", "alert"]` |
| `N-08` | `general` | Delete saved notification data | `Permanently deletes all stored notification history, Money transactions, and learned rules from this PC.` | Danger button with confirmation modal | `["delete", "clear", "wipe", "reset", "privacy"]` |

### 4.3 App Avatars & Deferred Real App Icons Strategy
- **Phase 1 (Stable Letter Avatars):** `NotificationsView` renders deterministic per-package colored letter avatars (`getAppAvatar(appLabel, packageName)` in `src/lib/notificationTidy.ts`):
  1. A stable FNV-1a hash of `packageName` selects from a fixed 10-color dark-theme palette (`APP_AVATAR_PALETTE`), with every background/foreground pair verified to meet WCAG AA `>= 4.5:1` contrast.
  2. The avatar letter is the uppercase first letter of the resolved `appLabel`; if `appLabel` is unresolved or begins with a digit, emoji, or symbol, it falls back to the first letter of the package's last segment.
  3. The same `AppAvatar` is shared across app group headers, filter chips, muted app rows, and desktop toast previews.
- **Why name-based APK icon extraction was dropped:** On-device testing showed that production APKs obfuscate resource file names (three popular apps tested had zero entries containing `ic_launcher` among thousands of `res/` files), causing name-based APK entry scanning to always miss while wasting ranged `adb` reads.
- **Deferred Future Task (Real Icons via Manifest + `resources.arsc`):** Real app icon resolution is deferred to a separate future task requiring binary `AndroidManifest.xml` + `resources.arsc` lookup to map the obfuscated icon resource ID to its actual APK entry path. Note that adaptive/vector-only icons (`<adaptive-icon>` / XML vector drawables without raster PNG/WebP fallbacks) cannot be rendered without a companion app.

---

## 5. Detailed Feature Specification

### 5.1 Tidy Notification View (Phase 1)

1. **Whitespace collapse & 2-line clamp:**
   - Normalize preview text by collapsing `\r\n` and consecutive blank lines / redundant spaces into clean single line breaks (`normalizeNotificationText(text)`).
   - Clamp card body to 2 lines (`line-clamp-2`). When text exceeds the clamp threshold (or contains collapsed newlines), render a **"Show more"** / **"Show less"** button (`aria-expanded`).
   - Preserve raw uncollapsed text for the **Copy** button and expanded view.
2. **Same-app + same-title stacking:**
   - Within each date section and app group, cluster notifications sharing `(package, normalizedTitle)` (case-trimmed).
   - Display the newest notification in the stack on top with a **`+N more`** button (`aria-expanded`) that expands the older stacked items inline below it.
3. **Collapsible app groups:**
   - Each app header (`<button aria-expanded>`) collapses or expands that app's notifications within a date section, displaying a deterministic colored letter avatar, resolved app name, visible item count, and read-only retention + Money source badges.
4. **Time sections & relative times:**
   - Partition visible notifications by local calendar day into **Today**, **Yesterday**, and **Earlier** (headers skipped if empty).
   - Format relative times (`Just now`, `5m ago`, `2h ago`, `Yesterday · 18:40`, `Nov 21 · 09:15`) with full local date-time in `title` / `aria-label`.
5. **App filter chips & search:**
   - Horizontal filter bar above the list: `All` chip + one chip per active app (showing deterministic letter avatar + count). Selecting an app filters the feed to that app.
   - Search box (`role="search"`, Escape clears) matches case-insensitively against `appLabel`, `package`, `title`, and `text`.
6. **Sensitive app masking:**
   - Apps flagged `sensitive: true` render a masked placeholder (`"Sensitive content hidden — click to reveal"`) in place of `title` and `text` (and `"New notification from <App>"` in desktop toast previews).
   - Clicking **Reveal** (`aria-expanded`) unmasks that item or app in-memory only; state automatically re-masks on switching sub-tabs/tabs, device disconnect, or clicking **Mask**.
   - Opt-in convenience: when an app is set to `Transactions only`, marked as a Money source, or newly marked `Sensitive`, default its `sensitive` flag to `true` (user-toggleable).
7. **Local "Hide" action (strictly read-only):**
   - Each notification (and stack) offers a **Hide** icon button (`"Hide in AnyLeap"`).
   - Hidden items are suppressed locally using `(package, key, postedUnix)` without ever sending any dismiss command to Android (`"Read-only — replies and dismisses stay on the phone"`).
   - A compact `"N hidden · Show"` control restores locally hidden items.

### 5.2 Per-App Retention, Money Sources (`0..n`) & Deduplication (Phase 2)

**Retention policy** and **Money sources** are **separate settings** per `(package)`:

1. **Notification retention policy (`retention`):**

| Policy | Code value | Persisted to disk? | Notification retention window | Raw text / names stored? | Eligible for Money? |
|---|---|---|---|---|---|
| **Don't keep** *(default)* | `"none"` | No (memory-only) | Session only | Never on disk | **No** |
| **Keep history** | `"history"` | Yes (`HistoryVault`) | `7`, `30` *(default)*, or `90` days | Yes (DPAPI-encrypted) | **Only if ≥ 1 enabled Money source** |
| **Transactions only** | `"transactions"` | Yes (`TransactionsVault`) | N/A (no notification stored; transactions follow §5.2.3) | **Never** | **Only if ≥ 1 enabled Money source** |

2. **Money sources (`moneySources`: `0..n` per app):**
   - Each app (`package`) holds an array `moneySources: MoneySourceConfig[]` (default `[]` = zero Money sources).
   - Each `MoneySourceConfig` has its own `id` (unique within the app), `type` (`"bank" | "ewallet" | "paylater" | "credit_card"`), `ruleTemplate` (rule set identifier, e.g. `"generic_bank"`, `"generic_ewallet"`, `"generic_paylater"`, `"generic_credit_card"`, `"cobranded_card_in_shop_app"`, `"custom"`), `enabled` (`boolean`), and optional `label`.
   - **Why `0..n` per app:** A single app often serves multiple financial roles (e.g., a shopping app that also issues a co-branded credit card and/or paylater line, or a bank app that notifies for both a checking account and a credit card).
   - **Strict Money gating & per-notification rule scoping:** Only apps where `retention !== "none"` **and** at least one `source` in `moneySources` has `enabled === true` enter the transaction parser. Within such an app, **each notification is classified individually against the configured sources' rule sets**. Notifications matching **no** source rule (e.g., shopping promotions with product prices) never count and produce zero records.
   - **Other non-source apps:** Apps with `moneySources = []` — including pure shopping apps, email apps, retail membership apps, and credit-score apps — **never contribute to Money totals**, even when their notification text contains currency amounts or the app is set to `Keep history`.
   - **Email apps:** Configured with `moneySources = []` by default. Optional email-receipt parsing may be introduced later as an opt-in source template.
   - **UX convenience (`"Add money source"` in `N-04`):** Clicking **"Add money source"** on an app row opens an inline multi-type selector with checkboxes for **Bank**, **E-wallet**, **Paylater**, and **Credit card** (with helper text *"Many wallets also offer paylater — tick both if you use both."*). Saving creates one source per ticked type, each using its generic rule template (`Generic bank`, `Generic e-wallet`, `Generic paylater`, `Generic credit card` — template IDs stay internal) and a default `<App label> <Type>` label that can be edited later. `"Custom"` is hidden from the selector for now while the data model retains `"custom"` rule support; a rule editor (*"create a rule from an example notification"*) is planned later. Adding a source when `retention === "none"` auto-selects `"transactions"` and defaults `sensitive` to `true`. Setting an app to `"transactions"` with `moneySources = []` prompts the user to add at least one Money source, and having `≥ 1` enabled Money source while `retention === "none"` shows an inline warning (*"Set this app to Transactions only (or Keep history) so Money can count it."*) with a one-click **"Use Transactions only"** button.

3. **Separate Transaction Retention (`transactionRetention`):**
   - Global setting in `N-05`: **`1 year` (365 days, default)**, **`2 years` (730 days)**, or **`Forever`**.
   - **Independence invariant:** Parsed `StoredTransaction` and `LiabilityEvent` records derived from `Keep history` apps are governed **exclusively** by `transactionRetention`. Expiring or manually purging a `Keep history` app's `StoredNotification` after `7`/`30`/`90` days **never** deletes its derived `StoredTransaction` or `LiabilityEvent` records.

4. **Deduplication:**
   - Composite key `(hw_serial, key, posted_unix)` (where `hw_serial` comes from `resolve_device_identity`, falling back to `serial` only if hardware serial is unavailable).
   - Polling the same active notification every 10 s produces the same `(hw_serial, key, posted_unix)` → ignored as duplicate.
   - If Android updates an existing notification `key` in place with a newer `postTime` (`posted_unix` changes), the new `(hw_serial, key, posted_unix)` is evaluated once.
   - **Master gate (`historyEnabled`):** When `false` (default), **nothing** is persisted to disk for any app.
   - **Transactions-only & identifier privacy guarantee:** Card numbers and account numbers are **never** stored in `TransactionsVault` (and for apps set to `"transactions"`, `title`, `text`, `bigText`, sender names, and card/account numbers exist **only** transiently in memory during the poll cycle, are never serialized to logs or disk, and are dropped immediately after extracting `StoredTransaction` / `LiabilityEvent` including `similarity_hash` and `sub_account` computed in memory using `TransactionsVault.rule_salt`).

### 5.3 Encrypted Local Store (Phase 2 — see ADR-0007)

- Stored under `app_local_data_dir()` (`%LOCALAPPDATA%\<bundle-id>\` on Windows — never roaming `%APPDATA%` or `settings.json`):
  - `phone_hub_history.v1.dpapi` — DPAPI-encrypted `HistoryVault`
  - `phone_hub_transactions.v1.dpapi` — DPAPI-encrypted `TransactionsVault` (stores `rule_salt`, `transactions`, `manual_overrides`, and `correction_rules`)
- Encrypted via Windows DPAPI `CryptProtectData` / `CryptUnprotectData` (`CRYPTPROTECT_UI_FORBIDDEN`, current-user scope `dwFlags = 0`, optional entropy `b"AnyLeap.PhoneHub.Vault.v1"`).
- Written atomically (`write temp -> flush -> rename`).
- Dirty-checked: disk writes happen **only** when ingestion adds at least one new record, matching/pruning mutates state, the user applies a correction, or the user deletes data — never on idle 10 s polls.
- **Delete actions:**
  - **Per-app delete:** removes all history and transaction records matching `package` (and optionally learned rules for `package`, clearly labeled) and re-encrypts the vault.
  - **Delete all:** removes both `.dpapi` files, clears in-memory history/transactions/rules caches, and resets dedupe indices.

### 5.4 Transaction Parser, Rule Format, `sub_account` Keys & Amount-less Liability Events (Phase 3)

1. **External JSON rule format (`TransactionRulePack` v1):**
   - Built-in generic rules and rule templates live in a bundled JSON asset (`src-tauri/resources/transaction_rules.default.json`), **not** hardcoded bank/brand names in Rust.
   - Optional user overrides/extensions live in `<app_local_data_dir>/transaction_rules.user.json` (loaded and merged ahead of defaults; invalid rules are skipped with a non-sensitive diagnostic warning naming only the rule `id`).
   - **Per-notification, per-source evaluation:**
     - Only apps where `retention != "none"` and at least one `source` in `moneySources` has `enabled == true` enter the parser.
     - For each notification, candidate rules are gathered across the app's enabled `MoneySourceConfig` entries: for each enabled `source`, select rules matching `(package == source_pkg || rule.ruleTemplate == source.ruleTemplate)` and `source.type in rule.sourceTypes` (plus `"*"` fallback rules **only** when `source.ruleTemplate` opts into generic fallback, e.g., `generic_bank` / `generic_ewallet` / `generic_paylater` / `generic_credit_card` — **never** for mixed-purpose templates such as `cobranded_card_in_shop_app`).
     - Order candidates by specificity (`exact package` > `ruleTemplate` > `"*"` generic), then `priority` descending; **first match wins** and binds the emitted record to that winning source's `(source_id, source_type)`.
     - **Zero-count default for unmatched alerts:** If **no** rule matches the notification (e.g., a shopping app promotion containing a product price), the parser emits **nothing** — it never falls back to guessing amounts from arbitrary text.
2. **Locale number parsing (`parse_localized_amount`):**
   - Supports `numberFormat`: `"auto"` | `"dot_decimal"` | `"comma_decimal"`.
   - Examples with fictional amounts:
     - `"1,021,403.00"` (`dot_decimal` or `auto`) → `102140300` (`scale = 2`).
     - `"1.021.403,00"` (`comma_decimal` or `auto`) → `102140300` (`scale = 2`).
     - `"45.000"` under `comma_decimal` (or `auto` when followed by exactly 3 digits and Indonesian currency like `Rp` / `IDR`) → `4500000` (`scale = 2`).
     - `"120000"` → `12000000` (`scale = 2`).
   - Amounts are stored as exact integer minor units (`i64`) + `scale: u8` (default `2`), never floating-point `f64`, preventing rounding drift in sums.
3. **Transient pattern extraction & opaque `sub_account` identifier (zero raw numbers or text persisted):**
   - Rules may optionally capture `counterparty` (merchant/VA/payee token), `billDue` (statement due amount on `paylater`/`credit_card` notifications), or `cardLast4` (masked card/account token or trailing 4 digits), and tag `transferHint` (`"virtual_account"` | `"top_up"` | `"payment"` | `"bill"` | `"repayment"`) and `eventKind` (`"transaction"` default | `"bill_paid"`).
   - **Never store raw counterparties:** `counterparty` is normalized in memory (`lowercase`, trim, collapse whitespace, strip variable digits/reference tails where configured) and immediately hashed with the vault's 32-byte random `rule_salt` (`SHA-256(rule_salt || package || "::" || direction || "::" || normalized_pattern)`) into `similarity_hash`, then the raw string is dropped.
   - **Never store card or account numbers (`sub_account` key):** To distinguish multiple cards or accounts within the same app/source without persisting card numbers, masked numbers, or even raw last-4 digits:
     1. In memory during parsing, extract only the trailing 4 ASCII digits (`last4`) from `cardLast4` and discard the full/masked string immediately.
     2. Compute `sub_account = "sa_" + hex(SHA-256(rule_salt || "sub_account::" || package || "::" || source_id || "::" || last4))`.
     3. Immediately drop `last4` from memory. Only the opaque `sub_account` hash is stored on `StoredTransaction` / `LiabilityEvent`.
     4. **User nicknames:** Users may assign a friendly nickname (e.g. `"Everyday Card"`, `"Work Card"`) to any `sub_account` key; nicknames are stored in `TransactionsVault.sub_account_nicknames` (DPAPI-encrypted, surviving age pruning). Before a user sets a nickname, `MoneyView` displays a non-identifying deterministic label such as `"Card #1"`, `"Card #2"` (never revealing digits).
4. **Amount-less liability events (`eventKind: "bill_paid"`):**
   - When a rule with `eventKind: "bill_paid"` matches a `paylater` or `credit_card` notification that has **no amount** (for example, fictional `"Credit card bill payment for card 4111-22**-****-0042 succeeded on 25 Nov"`), the parser does **not** fabricate a zero-amount `StoredTransaction`. Instead, it emits a `LiabilityEvent` with `eventType: "bill_paid"`, `package`, `source_id`, `source_type`, `sub_account` (derived from `0042`), `timestamp_ms`, and `status: "unmatched_needs_review"`, which enters Pass 2 of the §5.5 matcher.
5. **Shipped generic rules & mixed-purpose examples (fictional fixtures only, `AGENTS.md` §1.3):**
   - English Out: `"You spent IDR 120,000.00 at Groceries."` → `direction: "out"`, `currency: "IDR"`, `amountMinor: 12000000`, `category: "Groceries"`.
   - English In: `"You received IDR 2,500,000.00 from Payroll at Income."` → `direction: "in"`, `currency: "IDR"`, `amountMinor: 250000000`, `category: "Income"` (sender name `"Payroll"` is hashed into `similarity_hash` and raw text is **discarded**, never stored).
   - Indonesian Out (`keluar` / `debit` / `pembayaran` / `transfer`): `"Pembayaran Rp 45.000 di Makan"` / `"Transaksi debit IDR 1.021.403,00 di Belanja"` → `direction: "out"`.
   - Bank Payment / VA / Bill / Top-up hints: `"Payment OUT Rp 245.500,00 — Card Bill"` (`transferHint: "payment"` or `"bill"`), `"Transfer ke Virtual Account Rp 250.000"` (`transferHint: "virtual_account"`), `"Top up e-wallet Rp 100.000"` (`transferHint: "top_up"`).
   - Indonesian In (`masuk` / `kredit` / `diterima`): `"Transfer masuk Rp 1.021.403,00"` / `"Dana masuk IDR 500,000.00"` → `direction: "in"`.
   - Co-branded credit card inside a shopping app (`cobranded_card_in_shop_app` template):
     - Purchase: `"Credit card transaction of Rp 245.500,00 at Sample Roastery on 25/11"` → `sourceType: "credit_card"`, `direction: "out"`, `amountMinor: 24550000`, `currency: "IDR"`.
     - Amount-less bill paid: `"Credit card bill payment for card 4111-22**-****-0042 succeeded on 25 Nov"` → `LiabilityEvent` (`eventType: "bill_paid"`, `sub_account` hashed from `0042`, no amount).
     - Promotion in the exact same app: `"Flash Deal! Wireless Mouse Rp 245.500,00 today only"` → matches **no** rule in `cobranded_card_in_shop_app` → **ignored** (zero Money records).
   - Liability statement due hint: `"Tagihan jatuh tempo IDR 1.500.000,00"` → records `statementDueMinor` on the liability source without double-counting as a new expense.
6. **Category fallback:** When a rule has no `category` capture group, it uses the rule's static `defaultCategory` (e.g. `"Transfer"`, `"Spending"`, `"Income"`, `"Debt payment"`), falling back to `"Uncategorized"`.

### 5.5 Self-Transfer, Debt-Payment & Amount-less Bill-Paid Matching Algorithm (Phase 3)

Transfers between the user's own accounts must not inflate In or Out totals, and amount-less bill-paid confirmations must link cleanly to the corresponding liquid outflow.

1. **Definitions:**
   - **Asset sources:** `"bank"` and `"ewallet"`.
   - **Liability sources:** `"paylater"` and `"credit_card"`.
   - **Distinct source identity:** Two events come from different Money sources whenever `(a.package, a.source_id) != (b.package, b.source_id)` (allowing two distinct sources inside one app, e.g. `bank` + `credit_card`, to match each other while preventing same-source self-matches).
   - **Transfer between your accounts (`classification: "transfer"`):** An `out` on an asset source paired with an `in` on a **different** asset source. Listed in the transaction feed, **excluded from In/Out totals** in both **Spending** and **Cash flow** views.
   - **Debt payment (`classification: "debt_payment"`):** An `out` on an asset source (`bank` / `ewallet`) paired with either (a) an amounted `in` (repayment confirmation) on a liability source (`paylater` / `credit_card`), (b) an amount-less `bill_paid` `LiabilityEvent` on a liability source, **or** (c) manually/rule-marked as debt payment. Excluded from **Spending** totals (to prevent double-counting purchases) and **included in Cash flow** outflows; reduces the target liability's outstanding balance by the matched `OUT` amount.
   - **Possible transfer (`classification: "possible_transfer"`):** An unmatched `out` from a `"bank"` source whose parser metadata has `transferHint` `"virtual_account"` or `"top_up"`. Flagged with a **"Review"** badge (`needsReview = true`) so the user can confirm (`Mark as transfer`, `Mark as debt payment`, or keep as expense).
   - **Bill paid (amount unknown) (`LiabilityEventStatus::UnmatchedNeedsReview`):** An amount-less `bill_paid` `LiabilityEvent` that matched no asset `OUT` within `±W`. Recorded and surfaced in `MoneyView` as **`"Bill paid (amount unknown)"`** with a **"Review"** flag so the user can manually link it to an `OUT`, or dismiss/ignore it.

2. **Two-Pass Deterministic Matching Algorithm (One-to-One, Closest-Time):**
   - **Inputs:** All non-ignored transactions not locked by a manual per-transaction override (`manualClassification == null`), plus all active `LiabilityEvent` (`bill_paid`) records not manually locked/ignored.
   - **Window:** `W = transferMatchWindowMinutes * 60 * 1000` ms (default `15 * 60 * 1000` = `900,000` ms, configurable via `N-06`).
   - **Precedence rule (Pass 1 before Pass 2) & justification:**
     - **Exact-amount matching (Pass 1) always runs before amount-less `bill_paid` matching (Pass 2).**
     - *Why:* A Pass 1 match requires exact currency and exact `amount_minor` equality on *both* sides within `±W`, which is a much higher-precision signal than an amount-less `bill_paid` event (which has no amount constraint and can match any asset `OUT` in `±W`). If Pass 2 ran first, an amount-less `bill_paid` could greedily consume a bank `OUT` that already has an exact-amount e-wallet `IN` partner, misclassifying a real bank→e-wallet transfer as a debt payment and leaving the e-wallet `IN` orphaned as **false Income**. Running Pass 1 first prevents phantom Income and ensures amount-less `bill_paid` events only bind to outflows not already accounted for by an exact two-sided transfer/repayment.
     - *Within Pass 1 tie-break:* If a single `asset OUT` has both an exact-amount `asset IN` and an exact-amount `liability IN` within `±W`, closest `|dt|` wins; on exact `|dt|` equality, `liability IN` (`DebtPayment`) takes precedence over `asset IN` (`Transfer`) because liability repayments are explicitly bound to a debt source.

   - **Pass 1 — Exact-Amount Matching (`asset OUT` ↔ `asset IN` or amounted `liability IN`):**
     - **Candidate generation:** Collect every pair `(out_tx, in_tx)` where:
       - `out_tx.direction == "out"` and `in_tx.direction == "in"`
       - `out_tx.source_type` is `"bank"` or `"ewallet"` (asset outflow)
       - `(out_tx.package, out_tx.source_id) != (in_tx.package, in_tx.source_id)` (different money sources)
       - `out_tx.currency == in_tx.currency` (same currency)
       - `out_tx.amount_minor == in_tx.amount_minor && out_tx.scale == in_tx.scale` (exact same amount)
       - `|out_tx.timestamp_ms - in_tx.timestamp_ms| <= W`
     - **Deterministic sort:** Sort candidate pairs ascending by the 5-tuple key:
       1. `dt = (out_tx.timestamp_ms - in_tx.timestamp_ms).abs()` (closest timestamp first).
       2. `liability_tie = if matches!(in_tx.source_type, Paylater | CreditCard) { 0 } else { 1 }` (on exact `|dt|` tie, `DebtPayment` wins over `Transfer`).
       3. `lag_penalty = if in_tx.timestamp_ms >= out_tx.timestamp_ms { 0 } else { 1 }` (tie-break: prefer `IN` arriving at or after `OUT`).
       4. `out_tx.timestamp_ms` ascending.
       5. `(out_tx.id, in_tx.id)` lexicographically ascending (total deterministic order).
     - **Greedy one-to-one assignment:** Maintain `matched_tx_ids: HashSet<String>`. For each candidate `(out_tx, in_tx)` in sorted order:
       - If `matched_tx_ids.contains(&out_tx.id) || matched_tx_ids.contains(&in_tx.id)`, skip.
       - Insert both IDs into `matched_tx_ids`, compute `pair_id = deterministic_pair_id(&out_tx.id, &in_tx.id)`.
       - If `in_tx.source_type` is `"paylater"` or `"credit_card"`: mark `out_tx` as `classification = "debt_payment"` (and `in_tx` as `repayment_received` linked via `pair_id`, reducing the liability source's — and matching `sub_account`'s — outstanding balance).
       - Else (`in_tx.source_type` is `"bank"` or `"ewallet"`): mark both `out_tx` and `in_tx` as `classification = "transfer"`, `transfer_pair_id = Some(pair_id)`.

   - **Pass 2 — Amount-less `bill_paid` Matching (`LiabilityEvent` ↔ remaining unmatched `asset OUT`):**
     - **Candidate generation:** Collect every pair `(out_tx, event)` where:
       - `out_tx.id` is **not** in `matched_tx_ids` (and not locked by `manual_classification` or `"ignored"`), `out_tx.direction == "out"`, and `out_tx.source_type` is `"bank"` or `"ewallet"`.
       - `event.event_type == "bill_paid"` and `event.status` is not manually locked/ignored.
       - `(out_tx.package, out_tx.source_id) != (event.package, event.source_id)`.
       - `|out_tx.timestamp_ms - event.timestamp_ms| <= W`.
     - **Preference & deterministic sort:** Sort candidate pairs ascending by the 5-tuple key:
       1. `hint_tier`: `0` if `out_tx.transfer_hint` is `Some(Payment | VirtualAccount | Bill | Repayment)` (or `out_tx.category` is `"Payment"` / `"Bill"` / `"Debt payment"`), else `1`. *(Prefer lines labelled payment, virtual account, or bill over unlabeled outflows.)*
       2. `dt = (out_tx.timestamp_ms - event.timestamp_ms).abs()` ascending *(closest in time)*.
       3. `lag_penalty = if event.timestamp_ms >= out_tx.timestamp_ms { 0 } else { 1 }` *(bill confirmation usually arrives at or after the bank/e-wallet OUT)*.
       4. `out_tx.timestamp_ms` ascending.
       5. `(out_tx.id, event.id)` lexicographically ascending.
     - **Greedy one-to-one assignment:** Maintain `matched_event_ids: HashSet<String>`. For each candidate `(out_tx, event)` in sorted order:
       - If `matched_tx_ids.contains(&out_tx.id) || matched_event_ids.contains(&event.id)`, skip.
       - Insert `out_tx.id` into `matched_tx_ids` and `event.id` into `matched_event_ids`.
       - Set `out_tx.classification = "debt_payment"`, `out_tx.linked_liability_event_id = Some(event.id)`, `out_tx.linked_liability_source_id = Some(event.source_id)`, `out_tx.linked_sub_account = event.sub_account.clone()`.
       - Set `event.status = "linked"`, `event.linked_tx_id = Some(out_tx.id)`, `event.resolved_amount_minor = Some(out_tx.amount_minor)`, `event.resolved_currency = Some(out_tx.currency)`, `event.scale = Some(out_tx.scale)`.
       - That `DebtPayment` amount reduces the target liability's (and `event.sub_account`'s, when present) outstanding balance.

   - **Pass 3 — Fallback Review Flags:**
     - For every remaining unmatched `out_tx`:
       - If `out_tx.source_type == "bank"` and `out_tx.transfer_hint` is `Some(VirtualAccount | TopUp)`, set `classification = "possible_transfer"`, `needs_review = true`.
       - Otherwise `classification = "normal"`.
     - For every remaining unmatched `LiabilityEvent` (`bill_paid`):
       - Set `status = "unmatched_needs_review"` (`"Bill paid (amount unknown)"`, `needs_review = true`).

### 5.6 Paylater & Credit Card Liabilities, Views & Comparisons (Phase 4)

1. **Two Dashboard Views in `Phone → Money`:**

| Transaction Type | **Spending** *(default — for finding waste)* | **Cash flow** *(actual liquid money movement)* |
|---|---|---|
| Bank / E-wallet **In** (income) | Included in **In** | Included in **In** |
| Bank / E-wallet **Out** (normal expense) | Included in **Out** | Included in **Out** |
| Paylater / Credit card **Out** (purchase) | **Included in Out** (expense at purchase time) | **Excluded** (liquid cash did not leave yet) |
| **Debt payment** (Bank/E-wallet → Paylater/Credit card) | **Excluded** (avoids double-counting purchase) | **Included in Out** (cash left liquid account) |
| **Transfer between your accounts** (Bank ↔ E-wallet) | **Excluded** (listed in feed with badge) | **Excluded** (listed in feed with badge) |
| **Possible transfer** | Included in Out with **"Review"** flag | Included in Out with **"Review"** flag |
| **Ignored** | **Excluded** | **Excluded** |

2. **Per-Liability & Per-Sub-Account Tracking (`paylater` and `credit_card` sources):**
   - For each liability source `(package, source_id)` — and per `sub_account` key when cards/accounts are distinguished — compute **outstanding balance**:
     - `outstanding_minor = max(0, sum(purchases) - sum(repayments))` (plus signed net indicator if repayments exceed captured purchases due to pre-existing history).
     - `sum(repayments)` counts each `RepaymentReceived` and each linked `bill_paid` `LiabilityEvent` (`resolved_amount_minor`) **once** (never double-counting the paired `DebtPayment` `OUT`).
     - When some transactions on a liability source carry a `sub_account` key and others lack card digits (`sub_account = None`), show the overall source balance plus per-sub-account rows (labeled with the user's nickname or deterministic `"Card #1"` / `"Card #2"` fallback, never digits) and an `"Unassigned"` row if needed.
     - When a liability notification states a **bill / due amount** (`statementDueMinor` + optional `statementDueUnix`), display the stated bill alongside the calculated running balance.
   - **Due next month vs Income:** Show a dedicated comparison card/bar comparing **Total income this month** against **Total liabilities due** (sum of latest stated bill amounts or current outstanding balances across liability sources), displaying remaining headroom (`income - liabilities_due`) or a warning when liabilities exceed monthly income.

3. **Comparisons & "Where did spending grow" (Phase 4):**
   - **Month-over-Month (MoM):** Compare current calendar month (or selected month) vs previous calendar month per `category` (absolute delta and `%` change).
   - **Year-over-Year (YoY):** Compare current calendar month vs the same calendar month 12 months earlier per `category`. When `< 13` months of data exist (e.g., under the `1 year` retention default or newly started vault), show `"Not enough history for YoY (requires > 12 months)"`.
   - **"Where did spending grow":** Ranked callout card showing the top categories (e.g. top 5) with the largest positive spending increase (`current_period - previous_period > 0`) under the **Spending** view.

### 5.7 Manual Corrections with Salted-Hash Memory (Phase 4)

1. **Per-transaction actions:** From any row in `MoneyView`, the user can choose:
   - **Mark as transfer** (`"transfer"`)
   - **Mark as debt payment** (`"debt_payment"`)
   - **Ignore** (`"ignored"`)
   - **Change category** (select or type category)
   - **Restore automatic classification** (clears manual override)
2. **"Apply to similar in the future" toggle:**
   - When checked, AnyLeap creates/updates a `CorrectionRule` keyed by `(package, direction, similarity_hash)` where `similarity_hash` was computed at parse time from `SHA-256(rule_salt || package || "::" || direction || "::" || normalized_counterparty_or_va)`.
   - **Privacy invariant:** Raw merchant names, sender names, and virtual-account numbers are **never** stored in the correction rule or transaction record — only the salted hash.
   - **Retroactive + future application:** Saving a rule with `"Apply to similar"` updates existing matching transactions in `TransactionsVault` and automatically applies to all future incoming transactions sharing that `(package, direction, similarity_hash)`.
   - **Survives pruning:** `manual_overrides` (for retained transactions) and `correction_rules` (always) live in `TransactionsVault` and are **never deleted** when old transactions expire under `1 year` / `2 years` retention.

### 5.8 Money Dashboard Layout (`Phone → Money`, Phases 3 & 4)

- **Disclaimer banner (always visible):** `"Estimate only — calculated from captured phone notifications while connected. Cash, missed notifications, and unrecognized formats are excluded."`
- **Multi-currency rule:** Never sum different currencies together. Default to the most recent/most frequent currency; render a currency switcher pill when `> 1` currency exists.
- **Phase 3 (Sources, Parser, Transfer/Bill Matching & Money Basic):**
  - **View empty/setup state:** When `historyEnabled === false` or zero apps have an enabled Money source, show a clear CTA linking to **Settings → General** (`N-01` / `N-04`).
  - **Summary cards:** **Today** (since local `00:00`), **This week** (Monday `00:00` local), and **This month** (1st of month `00:00` local), each showing **In**, **Out**, and **Net** (`In - Out`) with matched transfers excluded.
  - **Large income highlight:** When `largeIncomeThreshold > 0`, incoming transactions `>= threshold` receive a `"Large income"` gold badge (`--gold-400`) and a top highlight strip.
  - **Review queue pill:** Surface count of **Possible transfer** rows and unmatched **`"Bill paid (amount unknown)"`** `LiabilityEvent` items (`"N items to review"`) so the user can inspect, link, or reclassify them quickly.
  - **Filters & list:** Filter by period (`Today` / `This week` / `This month` / `All`), direction/status (`All` / `In` / `Out` / `Transfers` / `Debt payment` / `Needs review`), and Money source (`All sources` + configured sources, with optional sub-account chip/nickname). Rows show relative/local timestamp, app icon + label + source-type pill + optional sub-account nickname/tag, category, classification badge (`Transfer between your accounts`, `Possible transfer`, `Debt payment`, `Bill paid (amount unknown)`), and signed formatted amount (or `Amount unknown` for unlinked `bill_paid` events; zero raw notification text, sender names, or card/account numbers).
- **Phase 4 (Liabilities, Views, Comparisons, Corrections & Export):**
  - **View switcher:** **Spending** *(default)* vs **Cash flow** segmented toggle.
  - **Liabilities & "Due next month" panel:** Per-liability source and per-`sub_account` cards (`Paylater` / `Credit card` outstanding balance, inline **sub-account nickname** editor, stated bill/due amount if captured, and resolve/link action for any `"Bill paid (amount unknown)"` events) + **"Due next month"** vs **Income this month** comparison bar.
  - **Comparisons & "Where did spending grow":** MoM & YoY category delta table + top category increases callout.
  - **Daily trend chart:** Stacked/grouped bar chart of daily **In** vs **Out** over the active window (7 d / 30 d / This month), rendered in flat Ink & Gold / status tokens with a keyboard-focusable data table toggle or `aria-label` summary per bar.
  - **Category breakdown:** Ranked horizontal bars showing total and percentage share per `category` (switchable between **Spending (Out)** and **Income (In)**).
  - **Inline row correction popover:** `Mark as transfer` / `Mark as debt payment` / `Ignore` / `Change category` + `"Apply to similar in the future"`, plus `Link to outflow` / `Dismiss` on `"Bill paid (amount unknown)"` events.
  - **CSV export (`Export CSV`):** Exports the currently filtered transaction rows (`timestamp_iso,app_id,app_label,source_id,source_type,sub_account,direction,classification,amount,currency,category` — using opaque `sub_account` / user nickname, never card numbers). Uses SPEC-31 `transferFolders` (`prepare_receive_dir` or `saveDialog` when `askWhereToSave` is true), neutralizes CSV formula injection (`=`, `+`, `-`, `@`, `\t`, `\r` prefixed with `'`), warns `"Exported CSV files are not encrypted"`, and surfaces **"Show in folder"** (`revealItemInDir`).

---

## 6. Data Model (Persisted Shapes)

### 6.1 `settings.json` (`notificationStorage` key)

```ts
export type RetentionPolicy = "none" | "history" | "transactions";
export type RetentionDays = 7 | 30 | 90;
export type MoneySourceType = "bank" | "ewallet" | "paylater" | "credit_card";
export type RuleTemplateId =
  | "generic_bank"
  | "generic_ewallet"
  | "generic_paylater"
  | "generic_credit_card"
  | "cobranded_card_in_shop_app"
  | "custom";
export type TransactionRetention = "1y" | "2y" | "forever";

export interface MoneySourceConfig {
  id: string;                          // unique within the app, e.g. "src_main" or UUID
  type: MoneySourceType;               // "bank" | "ewallet" | "paylater" | "credit_card"
  ruleTemplate: RuleTemplateId | string; // rule set/template bound to this source
  enabled: boolean;                    // default: true
  label?: string;                      // optional user label (e.g. "Co-branded Card")
}

export interface AppNotificationPolicy {
  retention: RetentionPolicy;         // default: "none"
  retentionDays: RetentionDays;       // used when retention === "history"; default: 30
  moneySources: MoneySourceConfig[];  // default: []; 0..n Money sources per app
  sensitive: boolean;                 // default: false (auto-suggested true on adding a Money source / "transactions")
}

export interface NotificationStorageSettings {
  version: 2;
  historyEnabled: boolean;              // master opt-in; default: false
  backgroundCapture: boolean;           // watch notifications when Phone tab is closed; default: false
  transactionRetention: TransactionRetention; // default: "1y" (1 year / 2 years / Forever)
  transferMatchWindowMinutes: number;   // default: 15 (clamped 1..60)
  largeIncomeThresholdByCurrency: Record<string, number>; // minor units, e.g. { IDR: 100000000 }
  perApp: Record<string, AppNotificationPolicy>;          // keyed by Android package ID
}
```

### 6.2 Encrypted Local Store (`phone_hub_history.v1.dpapi` & `phone_hub_transactions.v1.dpapi`)

```rust
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MoneySourceType {
    Bank,
    Ewallet,
    Paylater,
    CreditCard,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TxClassification {
    Normal,
    Transfer,
    PossibleTransfer,
    DebtPayment,
    RepaymentReceived, // liability-side IN paired with an asset DebtPayment
    Ignored,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransferHint {
    VirtualAccount,
    TopUp,
    Payment,
    Bill,
    Repayment,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LiabilityEventType {
    BillPaid,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LiabilityEventStatus {
    Linked,
    UnmatchedNeedsReview, // surfaced as "Bill paid (amount unknown)"
    Ignored,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StoredNotification {
    pub dedupe_id: String,      // "{hw_serial}::{key}::{posted_unix}"
    pub hw_serial: String,
    pub key: String,
    pub package: String,
    pub app_label: String,
    pub title: String,
    pub text: String,
    pub posted_unix: i64,
    pub captured_unix: i64,
    pub hidden_local: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StoredTransaction {
    pub id: String,                          // deterministic hash of "{hw_serial}::{key}::{posted_unix}::{rule_id}"
    pub dedupe_id: String,                   // "{hw_serial}::{key}::{posted_unix}"
    pub hw_serial: String,
    pub package: String,
    pub source_id: String,                   // MoneySourceConfig.id on this package
    pub app_label: String,
    pub source_type: MoneySourceType,
    pub sub_account: Option<String>,         // opaque "sa_<sha256(rule_salt || ... || last4)>"; never raw digits
    pub direction: TxDirection,              // "in" | "out"
    pub amount_minor: i64,                   // >= 0, integer minor units
    pub scale: u8,                           // e.g. 2 for IDR/USD
    pub currency: String,                    // ISO-4217 uppercase, e.g. "IDR"
    pub category: String,                    // e.g. "Groceries", "Income"
    pub classification: TxClassification,    // computed by matcher or overridden by user/rule
    pub manual_classification: Option<TxClassification>,
    pub manual_category: Option<String>,
    pub transfer_pair_id: Option<String>,
    pub linked_liability_event_id: Option<String>,
    pub linked_liability_source_id: Option<String>,
    pub linked_sub_account: Option<String>,
    pub transfer_hint: Option<TransferHint>,
    pub similarity_hash: Option<String>,     // salted hash of normalized counterparty/VA; zero raw text
    pub statement_due_minor: Option<i64>,    // optional bill/due amount parsed from liability alert
    pub statement_due_unix: Option<i64>,     // optional due timestamp
    pub timestamp_ms: i64,
    pub rule_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LiabilityEvent {
    pub id: String,                          // deterministic hash of "{hw_serial}::{key}::{posted_unix}::{rule_id}"
    pub dedupe_id: String,                   // "{hw_serial}::{key}::{posted_unix}"
    pub hw_serial: String,
    pub package: String,
    pub source_id: String,
    pub app_label: String,
    pub source_type: MoneySourceType,        // Paylater | CreditCard
    pub event_type: LiabilityEventType,      // BillPaid
    pub sub_account: Option<String>,         // opaque salted hash of last 4 digits; never raw digits
    pub status: LiabilityEventStatus,        // Linked | UnmatchedNeedsReview | Ignored
    pub linked_tx_id: Option<String>,        // matched bank/ewallet OUT id
    pub resolved_amount_minor: Option<i64>,  // copied from linked_tx when Linked
    pub resolved_currency: Option<String>,
    pub scale: Option<u8>,
    pub manual_locked: bool,                 // true when user manually linked or ignored
    pub timestamp_ms: i64,
    pub rule_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CorrectionRule {
    pub id: String,
    pub package: String,
    pub direction: TxDirection,
    pub similarity_hash: String,             // keyed with TransactionsVault.rule_salt
    pub classification: Option<TxClassification>,
    pub category: Option<String>,
    pub created_unix: i64,
}
```

`TransactionsVault` persists: `{ version: 1, rule_salt: [u8; 32], transactions: Vec<StoredTransaction>, liability_events: Vec<LiabilityEvent>, sub_account_nicknames: HashMap<String, String>, correction_rules: Vec<CorrectionRule> }`.

### 6.3 Transaction Rules JSON (`TransactionRulePack` v1)

```json
{
  "version": 1,
  "rules": [
    {
      "id": "generic-en-spent-at",
      "enabled": true,
      "priority": 100,
      "package": "*",
      "ruleTemplate": "generic_bank",
      "sourceTypes": ["bank", "ewallet", "paylater", "credit_card"],
      "eventKind": "transaction",
      "direction": "out",
      "numberFormat": "auto",
      "targetField": "text",
      "pattern": "(?i)\\byou spent\\s+(?P<currency>[A-Z]{3}|Rp\\.?|\\$)\\s*(?P<amount>[0-9][0-9.,]*)\\s+at\\s+(?P<category>[\\p{L}0-9 &_-]{2,40})",
      "defaultCategory": "Spending",
      "transferHint": null
    },
    {
      "id": "cobranded-cc-bill-paid-no-amount",
      "enabled": true,
      "priority": 140,
      "package": "*",
      "ruleTemplate": "cobranded_card_in_shop_app",
      "sourceTypes": ["credit_card"],
      "eventKind": "bill_paid",
      "direction": null,
      "numberFormat": "auto",
      "targetField": "text",
      "pattern": "(?i)credit card bill payment for card\\s+(?P<cardLast4>[0-9*\\-xX ]{4,24})\\s+succeeded",
      "defaultCategory": "Debt payment",
      "transferHint": "repayment"
    }
  ]
}
```

### 6.4 Backend IPC Commands (Phases 2–4)
- *(Deferred future task)* `phone_get_app_icon(serial, package) -> Option<String>` (manifest + `resources.arsc` icon resolution)
- `phone_set_notification_storage_config(config)` (Phase 2, syncs `NotificationStorageSettings` to Rust poller/store)
- `phone_get_notification_history(hw_serial, limit, before_unix) -> Vec<StoredNotification>` (Phase 2)
- `phone_set_notification_hidden(dedupe_id, hidden) -> ()` (Phase 2)
- `phone_delete_app_notification_data(package, hw_serial?) -> DeleteResult` (Phase 2)
- `phone_delete_all_notification_data() -> DeleteResult` (Phase 2)
- `phone_get_transactions(hw_serial?, currency?, from_ms?, to_ms?) -> MoneyVaultSnapshot` (Phase 3, returns `transactions`, `liabilityEvents`, and `subAccountNicknames`)
- `phone_apply_transaction_correction(tx_id, classification?, category?, apply_to_similar) -> CorrectionResult` (Phase 4)
- `phone_resolve_liability_event(event_id, linked_tx_id?, ignore?) -> LiabilityResolveResult` (Phase 4, manually links or ignores an unmatched `"Bill paid (amount unknown)"` event)
- `phone_set_sub_account_nickname(sub_account, nickname?) -> ()` (Phase 4, stores or clears an encrypted nickname in `TransactionsVault`)
- `phone_export_transactions_csv(hw_serial?, currency?, view_mode?, from_ms?, to_ms?, dest_path?) -> PullResult` (Phase 4)

---

## 7. Privacy and Security (Threat Model)

| Threat | Mitigation & Residual Risk |
|---|---|
| **Screen sharing / shoulder surfing** | Apps marked `sensitive` mask titles and bodies behind a click-to-reveal button and suppress content in desktop toast previews (`"New notification from <App>"`). Ephemeral reveal state resets on tab change, disconnect, or manual re-mask. Local **Hide** removes sensitive rows from view without touching the phone. |
| **Other local Windows users / offline disk access** | Vault files live in per-user `%LOCALAPPDATA%` and are encrypted with Windows DPAPI `CryptProtectData` scoped to the current Windows user SID (`dwFlags = 0`, `CRYPTPROTECT_UI_FORBIDDEN`). Other local OS accounts and raw disk copies cannot decrypt the blobs. *Residual limit:* processes running under the **same** logged-in Windows user session can invoke `CryptUnprotectData`. |
| **Backup files / cloud profile roaming** | Storing `.dpapi` files in `%LOCALAPPDATA%` (not `%APPDATA%` Roaming) avoids enterprise roaming-profile sync; KVM never accesses these files. If an OS backup copies `.dpapi` files to a new PC without the user's Windows DPAPI master keys, blobs fail closed (`"Unable to decrypt local vault for this Windows user"`) and offer Delete & reinitialize. |
| **Diagnostic & debug logs** | `eprintln!`, activity timeline (`logActivity`), test harnesses, and KVM diagnostics must never include notification `title`, `text`, `bigText`, counterparty names, or transaction amounts. |
| **Transactions-only, card/account numbers & rule leakage** | Card numbers, masked card digits, and raw last-4 digits are **never** stored in `TransactionsVault`, `settings.json`, CSV exports, or logs; multi-card separation uses only the salted hash `sub_account = "sa_" + hex(SHA-256(rule_salt || "sub_account::" || package || "::" || source_id || "::" || last4))`. When `retention == "transactions"`, raw notification payloads are never persisted; counterparty/VA patterns are normalized in memory and stored solely as a salted hash (`SHA-256(rule_salt || ...)`) using a per-vault random salt inside the DPAPI vault. |
| **CSV export** | Exported CSVs are plaintext by user request; UI displays a clear `"Exported CSV files are not encrypted"` note and applies formula-injection prefix escaping (`'` before `=`, `+`, `-`, `@`, `\t`, `\r`). |

---

## 8. Retention and Deletion Semantics

1. **Two independent retention clocks:**
   - **Notification history (`HistoryVault`):** On app launch (when `historyEnabled` is `true`) and after each dirty poll batch, delete `StoredNotification` entries older than the app's configured `retentionDays` (`7 | 30 | 90`).
   - **Money transactions & liability events (`TransactionsVault`):** Governed strictly by `transactionRetention`:
     - `"1y"` *(default)*: prune `StoredTransaction` and `LiabilityEvent` entries older than 365 days.
     - `"2y"`: prune `StoredTransaction` and `LiabilityEvent` entries older than 730 days.
     - `"forever"`: never age-prune transactions or liability events.
   - **Independence:** Expiring a `StoredNotification` from a `Keep history` app **never** deletes `StoredTransaction` or `LiabilityEvent` records derived from it.
   - **Correction rules & sub-account nicknames survive pruning:** `CorrectionRule` and `sub_account_nicknames` entries in `TransactionsVault` are never expired by age-based transaction pruning; they are removed only when the user explicitly clears them, deletes that app's data, or triggers **Delete all**.
2. **Policy transitions:**
   - `history` → `transactions`: immediately purge `StoredNotification` entries for that `package` while **retaining** all `StoredTransaction` and `LiabilityEvent` entries.
   - `history` or `transactions` → `none`: prompt the user `"Also delete existing saved data for <App>?"` (default checked); confirming deletes that app's stored history, transactions, and liability events.
   - Disabling or removing a `MoneySourceConfig` on an app: stops parsing new notifications for that source; existing transactions remain until deleted or expired unless the user clicks **Delete** for that app.
   - Turning `historyEnabled` off (`N-01`): stops all new writes and hides Money/History until re-enabled; does **not** silently destroy existing vaults (user can re-enable or click **Delete all**).
3. **Hard limits:** Cap `HistoryVault` at `5,000` entries and `TransactionsVault` at `50,000` entries (oldest non-rule-protected transaction evicted first if cap is reached, with a non-blocking Settings notice if `Forever` nears the cap) to guarantee bounded disk size and sub-15 ms DPAPI operations.
4. **Local Hide vs Phone:** `hidden_local` affects only AnyLeap's rendering. Dismissing a notification on Android does **not** delete persisted `history` or `transactions` within their retention window.

---

## 9. Accessibility (WCAG AA, Keyboard, Reduced Motion)

- **Contrast (SPEC-26 Ink & Gold):** `--text-muted` (`#8B8FA3`) is placed only on `--ink-950` (`5.72:1`) or `--ink-900` (`5.15:1`), never on `--ink-800` (`4.24:1` fails AA); use `--text` (`#C9CBD6`, `8.40:1`) on `--ink-800`. Semantic In/Out/Transfer/Possible-transfer indicators pair color (`--status-success` / `--status-danger` / `--mist-300` / `--status-warning`) with `+` / `-` signs and explicit text/icon badges so color is never the sole cue.
- **Keyboard navigation:**
  - Filter chips, collapsible app headers (`aria-expanded`), stack expand buttons (`aria-expanded`), `"Show more"` / `"Show less"`, sensitive-content `"Reveal"` buttons, `"Hide"` actions, Spending/Cash flow view toggles, and row correction popovers are native `<button>` elements in logical DOM order with 2 px `--gold-400` `:focus-visible` outlines.
  - Search input supports `Escape` to clear.
  - Daily trend chart provides keyboard-focusable bar summaries (`aria-label` with date, In, Out, Net) plus an accessible table toggle/sr-only table.
- **Reduced motion:** All collapse/expand animations, bar transitions, and refresh spinners honor `@media (prefers-reduced-motion: reduce)` (`motion-reduce:transition-none`, `motion-reduce:animate-none`).

---

## 10. Performance (Poll Cadence and Store Size)

- **Poll cadence:** Reuses `PollTier::Notifications(notif_secs)` (`DEFAULT_NOTIF_POLL_SECS = 10`, `MIN_NOTIF_POLL_SECS = 5`). Background watching activates only when `phoneTabActive || desktopNotifications || (historyEnabled && backgroundCapture)`.
- **Zero idle disk writes:** An in-memory `HashSet<String>` of seen `(hw_serial, key, posted_unix)` dedupe IDs filters unchanged snapshots in memory. `CryptProtectData` + atomic file write runs **only** when a batch inserts ≥ 1 new item, updates transfer matching, applies a correction, or prunes expired records.
- **Store size & latency budget:** At ~500 bytes/notification and ~240 bytes/transaction, `5,000` history items (~2.5 MB) + `50,000` transactions (~12 MB worst case; typical 2-year personal volume ~5,000–10,000 items ≈ 1.2–2.4 MB) remain well within ~15 ms DPAPI encrypt/decrypt on desktop hardware.
- **Frontend rendering:** Paginate/window rendered notifications (initial 50 items + `"Show older"` / incremental batch) so 90-day history never blocks React rendering.
- **App avatars:** Pure client-side deterministic letter avatars in Phase 1 (zero background `adb` APK reads).

---

## 11. Phasing (Each Phase Independently Shippable)

1. **Phase 1 — Tidy Notification View & Deterministic Letter Avatars:**
   - Whitespace collapse, 2-line clamp + "Show more", same-app + same-title stacking, collapsible app groups, Today/Yesterday/Earlier sections + relative times, app filter chips, search, deterministic per-package colored letter avatars (real APK icons deferred), sensitive-app masking, local Hide (strictly read-only). Zero persisted notification payloads.
2. **Phase 2 — Per-App Retention Policy, `0..n` Money Sources & DPAPI Encrypted Local Store:**
   - Master opt-in (`N-01`), DPAPI vault (`phone_hub_history.v1.dpapi`, `phone_hub_transactions.v1.dpapi` shell with random `rule_salt`), `(hw_serial, key, posted_unix)` dedupe, independent per-app retention (`Don't keep` / `Keep history 7/30/90d` / `Transactions only`) and `0..n` Money sources (`Add money source` picking Type + Rule template), transaction retention (`1y` / `2y` / `forever`), Settings UI in `General` (`N-01`..`N-08`), per-app delete & Delete all, background capture toggle (`N-03`), history hydration in `NotificationsView`.
3. **Phase 3 — Per-Notification Parser, Salted `sub_account` Keys, Two-Pass Matching & Basic Money:**
   - Strict per-notification, per-source-rule classification (`app → 0..n moneySources`, unmatched alerts like shopping promos never count), JSON `TransactionRulePack` v1 + generic EN/ID & co-branded card rules + `parse_localized_amount` + user custom rules file, transient salted-hash `similarity_hash` and opaque `sub_account` keys (never storing card/account numbers), amount-less `bill_paid` `LiabilityEvent` extraction, two-pass greedy closest-time 1:1 matcher (Pass 1 exact-amount Transfer/DebtPayment; Pass 2 amount-less `bill_paid` linking within `±15 min` preferring `payment`/`virtual_account`/`bill` lines; Pass 3 **Possible transfer** and **`"Bill paid (amount unknown)"`** review flags), and `Phone → Money` basic summary, filters, and list.
4. **Phase 4 — Liabilities, Sub-Account Nicknames, Views, Comparisons, Corrections & Export:**
   - **Spending** *(default)* vs **Cash flow** views, **Paylater / Credit card** purchase-at-purchase-time + **Debt payment** classification + per-source and per-`sub_account` outstanding balance (with user nicknames and manual linking/resolution for `"Bill paid (amount unknown)"`) & **"Due next month"** vs monthly income, **Month-over-Month** & **Year-over-Year** category comparisons + **"Where did spending grow"**, inline manual corrections (`Mark as transfer`, `Mark as debt payment`, `Ignore`, category change) with `"Apply to similar in the future"` salted-hash rules surviving pruning, daily trend chart, category breakdown, and formula-safe CSV export via SPEC-31 `transferFolders`.

---

## 12. Test Plan

### 12.1 Rust Unit & Integration Tests (`cargo test`)
- **DPAPI vault (`#[cfg(windows)]`):** round-trip `CryptProtectData` / `CryptUnprotectData` with entropy; tamper detection returns error; wrong-entropy fails; atomic write leaves no partial file; `delete_app` and `delete_all` remove target records/files.
- **Retention & dedupe:**
  - `(hw_serial, key, posted_unix)` ignores identical poll repeats and records updated `postTime`.
  - Notification history pruning expires `7/30/90d` `StoredNotification` rows **without** deleting derived `StoredTransaction` rows.
  - Transaction retention pruning respects `1y` (365d), `2y` (730d), and `forever` (never prunes); `correction_rules` always survive pruning.
  - Enforces `transactions`-only zero-raw-text invariant and handles policy downgrades (`history` → `transactions` keeps transactions; `→ none` prompts/clears).
- **Money source gating, mixed-purpose apps & `sub_account` privacy (fictional fixtures only):**
  - **Non-source apps:** pure shopping promo, email receipt, credit-score, and retail membership notifications containing currency amounts produce **zero** `StoredTransaction` / `LiabilityEvent` entries when `moneySources == []`.
  - **Promo-from-money-app case:** configure `com.example.shop` with 1 source `{ id: "shop-cc", type: "credit_card", ruleTemplate: "cobranded_card_in_shop_app", enabled: true }` and `retention: "transactions"`. Feed:
    - Promo: `"Flash Deal! Running shoes Rp 245.500,00 today only"` → matches **no** rule → **0** transactions, **0** liability events.
    - Card purchase: `"Credit card transaction of Rp 245.500,00 at Sample Roastery on 25/11"` → **1** `StoredTransaction` (`source_type: CreditCard`, `direction: Out`, `amount_minor: 24550000`, `currency: "IDR"`).
  - **Two cards in one app (and zero raw card digits):** configure `com.example.shop` (or `com.example.bank`) with a `credit_card` source and feed:
    - Bill paid A: `"Credit card bill payment for card 4111-22**-****-0042 succeeded on 25 Nov"` → `sub_account = key_A`.
    - Bill paid B: `"Credit card bill payment for card 4111-22**-****-9012 succeeded on 25 Nov"` → `sub_account = key_B != key_A`.
    - Assert serialized `TransactionsVault` JSON contains **zero** occurrences of `"4111"`, `"0042"`, `"9012"`, or masked card strings, while `sub_account_nicknames` maps `key_A -> "Everyday Card"` and `key_B -> "Travel Card"` and survives age pruning.
  - **Multiple source types on one app:** configure `com.example.shop` with both `paylater` (`generic_paylater`) and `credit_card` (`cobranded_card_in_shop_app`); verify each notification routes strictly to the matching source (`source_id`, `source_type`).
  - English & Indonesian generic rules (`"You spent IDR 120,000.00 at Groceries."`, `"You received IDR 2,500,000.00 from Payroll at Income."`, `"Pembayaran Rp 45.000 di Makan"`, `"Transfer masuk Rp 1.021.403,00"`, `"Payment OUT Rp 245.500,00 — Card Bill"`, VA/top-up hints, liability statement due amounts).
  - Locale number formats (`1,021,403.00`, `1.021.403,00`, `45.000`, `12.50`, `12,50`).
  - Priority ordering (`package` > `ruleTemplate` > `"*"`), malformed regex in user file fails gracefully without crashing.
  - Salted `similarity_hash` verification: identical normalized counterparty/VA on same `(package, direction)` produces identical hash; zero raw counterparty strings exist in serialized `StoredTransaction` or `CorrectionRule`.
- **Two-pass Transfer, Debt-payment & Amount-less `bill_paid` matcher (explicit unit-test cases):**
  1. Exact match within ±15 min across two different asset sources (`bank` OUT + `ewallet` IN, same currency & amount) → both `Transfer`, excluded from In/Out totals.
  2. Outside configurable window (e.g., 15m 01s at 15 min setting; matched when window raised to 20 min) → respects window.
  3. Currency mismatch (`IDR 100,000` vs `USD 100,000`) → not matched.
  4. Same source (`(package, source_id)` identical) → not matched (while distinct `source_id`s inside the same `package` *can* match).
  5. One-to-one greedy closest-time: 2 identical `OUT`s (`12:00:00`, `12:08:00`) + 1 `IN` (`12:09:00`) → `12:08:00` pairs with `12:09:00`; `12:00:00` stays unmatched.
  6. Equal `|dt|` tie-break: `OUT` at `12:00:00`, `IN`s at `11:59:55` and `12:00:05` → `12:00:05` (`in >= out`) wins; further ties break deterministically by `out.timestamp_ms`, then `(out.id, in.id)`.
  7. Unmatched `bank` OUT with VA/top-up hint → `PossibleTransfer` (`needsReview`); unmatched normal `bank` OUT → `Normal`.
  8. Asset (`bank`/`ewallet`) OUT + amounted Liability (`paylater`/`credit_card`) IN → `DebtPayment` + `RepaymentReceived` (not generic `Transfer`).
  9. Manual per-tx override locks classification against re-matching; `"Apply to similar"` persists `CorrectionRule` and survives age pruning.
  10. **Bill paid with a matching bank OUT (label preference + closest-time):** `com.example.bank` posts a generic OUT (`Rp 120.000,00` at `10:04:00`) and a `Payment` OUT (`Rp 245.500,00`, `transferHint: Payment` at `10:02:00`); `com.example.shop` (`credit_card`) posts amount-less `bill_paid` for `sub_account` `key_A` at `10:05:00` → prefers the `Payment` OUT (`hint_tier = 0`) within ±15 min, marks it `DebtPayment` with `amount_minor = 24550000`, links `LiabilityEvent` (`Linked`), and reduces `key_A`'s (and source's) outstanding balance by `Rp 245.500,00` (and when two `Payment` OUTs exist at `10:01:00` and `10:03:00`, closest-time `10:03:00` wins 1:1).
  11. **Bill paid with no match:** `com.example.shop` posts amount-less `bill_paid` at `10:05:00` with either zero asset OUTs or only an asset OUT 25 min earlier (outside ±15 min) → `LiabilityEvent` stays `UnmatchedNeedsReview` (`"Bill paid (amount unknown)"`) and does not alter any balance.
  12. **Bank OUT that could match both a transfer and a bill (precedence):** `com.example.bank` posts `Payment` OUT `Rp 500.000,00` at `12:00:00`; `com.example.ewallet` posts `IN` `Rp 500.000,00` at `12:02:00`; `com.example.shop` (`credit_card`) posts amount-less `bill_paid` at `12:01:00`. Pass 1 pairs `bank OUT ↔ ewallet IN` as `Transfer` (taking precedence over Pass 2 so the e-wallet `IN` does not become false Income), while `bill_paid` falls back to any other unmatched `OUT` in ±15 min or becomes `"Bill paid (amount unknown)"` (`UnmatchedNeedsReview`) if none exists.
- **CSV export:** formula-injection escaping (`=`, `+`, `-`, `@`, `\t`, `\r`), `unique_dest_path` & `prepare_receive_dir` integration.

### 12.2 Local Real-Data Validation Harness (Implementation Phase, Local Only)
- Add an **ignored-by-default (`#[ignore]`)** Rust integration test in `src-tauri/src/phone_hub/` (run manually via `cargo test --manifest-path src-tauri/Cargo.toml -- --ignored real_capture_harness --nocapture`).
- Reads the user's DPAPI-encrypted notification captures from the file path provided in an environment variable (e.g., `ANYLEAP_REAL_CAPTURE_PATH`), skipping/failing cleanly if the env var is unset.
- Runs the transaction parser (using an in-memory source-type test mapping) and the transfer matcher against the decrypted records.
- **Strict zero-leak output rule:** Prints **only aggregate counts** to stdout (per source type: `parsed_in`, `parsed_out`, `transfers_matched`, `possible_transfers`, `unparsed_amount_lines`). **Never** prints or writes notification titles, bodies, app/bank names, counterparty names, or transaction amounts.
- All committed test fixtures in the repository remain 100% fictional (`AGENTS.md` §1.3).

### 12.3 Frontend Unit & Component Tests (Vitest)
- **Tidy view helpers & `NotificationsView`:** whitespace collapse, 2-line clamp + Show more toggle, same-app + same-title stacking (`+N more`), app collapse, Today/Yesterday/Earlier bucketing, filter chips + search, sensitive masking + ephemeral re-mask on unmount/tab change, local Hide + unhide, deterministic per-package letter avatars (stability, palette spread, >= 4.5:1 contrast, digit/emoji fallback), zero calls to any phone-dismiss command.
- **Settings (`GeneralSection` & `settingsFilter`):** `N-01`..`N-08` searchable by keywords, independent Retention vs `0..n` Money sources (`"Add money source"` picking Type + Rule template, enabling/disabling/removing individual sources), `1y` / `2y` / `Forever` transaction retention, configurable transfer window, single-home compliance, confirmation before Delete all.
- **`MoneyView` (Phases 3–4):**
  - Transfers excluded from Today/Week/Month In/Out/Net totals while visible in list; Possible transfers and `"Bill paid (amount unknown)"` events flagged for review.
  - **Spending** (default) includes Paylater/Credit card purchases and excludes Debt payments; **Cash flow** excludes Paylater/Credit card purchases and includes Debt payments.
  - Per-liability and per-`sub_account` outstanding balance (`purchases - repayments`, including two cards on one app), sub-account nickname editing (never showing raw digits), manual linking/dismissing for `"Bill paid (amount unknown)"`, stated bill/due display, and **"Due next month"** vs monthly income comparison.
  - MoM & YoY per-category comparisons (including graceful `< 13 months` YoY message) and **"Where did spending grow"** ranking.
  - Manual correction UI (`Mark as transfer`, `Mark as debt payment`, `Ignore`, category edit, `"Apply to similar in the future"`).
  - Multi-currency separation, large income highlight above threshold, trend/category calculations, disclaimer text present, CSV export button calls backend and renders `"Show in folder"`.
- **Compliance check:** grep/test verifying zero third-party app/bank/product names across all new docs, rule files, fixtures, and UI code (`AGENTS.md` §1.3).

### 12.4 Manual Verification (`docs/MANUAL-TEST-CHECKLIST.md`)
- Connect test Android device, toggle sensitive masking and local Hide, verify Android notification shade stays untouched.
- Enable `N-01`, configure one bank app (`Transactions only`, `Bank` source), one e-wallet (`Keep history 7d`, `E-wallet` source), and one shopping app (`Keep history 7d`, **not** a Money source); trigger fictional test notifications; verify shopping amounts do not enter Money, bank→e-wallet matching pairs as Transfer, restart AnyLeap, and verify `.dpapi` files contain zero plaintext strings.
- Keyboard-only walkthrough (filter chips, Show more, Reveal, Money view toggle, correction popover, Export CSV) and reduced-motion check.

---

## 13. Open Questions & Risks

1. **Pre-tracking credit card / paylater starting balance:** Because `outstanding = purchases - repayments`, liabilities carried over from before AnyLeap started capturing notifications can cause `purchases - repayments` to underestimate the real bill or go negative after a full repayment. *Mitigation:* prefer the stated bill/due amount whenever a statement notification captures `statementDueMinor`, floor displayed net-to-zero with a clear `"Calculated since tracking started"` helper, and consider an optional manual starting-balance adjustment if needed.
2. **One-sided repayment notifications:** Some paylater or credit card apps send no `"repayment received"` notification when paid from a bank app, so the bank outflow won't auto-pair with a liability IN. *Mitigation:* bank VA/top-up outflows become **Possible transfer** for 1-click review, and `"Mark as debt payment"` + `"Apply to similar in the future"` remembers that VA/counterparty hash permanently.
3. **Transfer window false positives / clearing delays:** Two unrelated identical small purchases within ±15 min across a bank and an e-wallet could false-match as a transfer, while slow inter-bank clearing (> 15 min) could miss matching. *Mitigation:* make the window configurable (`N-06`), allow single-transaction `"Restore automatic"` / `"Ignore"` / `"Mark as transfer"`, and keep tie-breaking deterministic.
4. **YoY comparisons vs default 1-year transaction retention:** With the default `1 year` (`365` days) transaction retention, data from 12+ months ago ages out right as YoY comparisons need it. *Mitigation:* surface a subtle note on the YoY column (`"Switch transaction retention to 2 years or Forever in Settings to keep full year-over-year history"`).
5. **APK icon extraction with obfuscated resource names & adaptive icons (deferred):** On-device verification showed that production APKs obfuscate `res/` entry names (three popular apps had zero `ic_launcher` entries among thousands of `res/` files), so name-based APK scanning fails and wastes ranged `adb` reads. *Mitigation:* Phase 1 uses deterministic per-package colored letter avatars; real icon resolution via binary `AndroidManifest.xml` + `resources.arsc` lookup is deferred to a separate future task, noting that adaptive/vector-only icons (`<adaptive-icon>` / XML vector drawables) cannot be rendered without a companion app.
6. **Android notification truncation/grouping:** Some apps replace individual notifications with a summary line (`"3 new transactions"`) when multiple alerts pile up before the next 10 s poll, which Android cannot expand retroactively via `dumpsys`. *Mitigation:* document this clearly in the Money disclaimer (`"based on captured notifications"`).
7. **Multi-currency large-income threshold UX:** Users receiving both `IDR` and `USD` need separate magnitude thresholds (`1,000,000` vs `100`). *Mitigation:* key `largeIncomeThresholdByCurrency` by currency code and default the Settings editor to the active/primary currency with an inline currency selector.
8. **Cross-device vs per-device Money view:** If a user switches phones, should Money default to the active phone or all phones? *Proposal:* default to the active phone (`PhoneView` consistency) with an optional `"All phones"` filter pill in `MoneyView` when > 1 `hw_serial` has saved transactions.
9. **Mixed-purpose app rule bleed (promotions vs card alerts):** Adding a co-branded credit card source to a shopping app could accidentally ingest promo/price alerts if broad `"*"` spending regexes ran on that app. *Mitigation:* require explicit rule-template scoping (`cobranded_card_in_shop_app` omits generic `"*"` fallback and uses strict card-context patterns), so unmatched promos drop silently with zero Money impact.
10. **Asymmetric card digits across purchase vs bill notifications:** A card purchase notification may omit card digits (`sub_account = None`) while its bill-payment confirmation includes masked last-4 digits (`sub_account = Some(key)`). *Mitigation:* track both source-level balance (`(package, source_id)`) and per-`sub_account` breakdown (with an `"Unassigned"` bucket when needed), and warn cleanly when a source mixes scoped and unscoped events.
11. **Amount-less `bill_paid` false linking:** Because `bill_paid` events carry no amount, paying a card via cash or an untracked bank within ±15 min of an unrelated e-wallet/bank purchase could falsely link the two. *Mitigation:* Pass 1 exact-amount transfers always run before Pass 2, Pass 2 prefers lines labelled `payment` / `virtual_account` / `bill`, and users can unlink or re-link any matched `LiabilityEvent` in `MoneyView`.
