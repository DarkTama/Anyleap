use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use rand::RngCore;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::phone_hub::poller::PhoneHubNotification;
use crate::phone_hub::PhoneHubState;

pub const HISTORY_VAULT_FILENAME: &str = "phone_hub_history.v1.dpapi";
pub const TRANSACTIONS_VAULT_FILENAME: &str = "phone_hub_transactions.v1.dpapi";
pub const VAULT_ENTROPY: &[u8] = b"AnyLeap.PhoneHub.Vault.v1";
pub const VAULT_DECRYPT_FAILED_PREFIX: &str = "VAULT_DECRYPT_FAILED:";
pub const HISTORY_CAPACITY: usize = 5_000;
pub const TRANSACTIONS_CAPACITY: usize = 50_000;
pub const VAULT_VERSION: u32 = 1;
const SECS_PER_DAY: i64 = 86_400;
const MS_PER_DAY: i64 = 86_400_000;

static ATOMIC_TEMP_SEQ: AtomicU64 = AtomicU64::new(1);

fn default_version_v2() -> u32 {
    2
}

fn default_vault_version() -> u32 {
    VAULT_VERSION
}

fn default_true() -> bool {
    true
}

fn default_retention_days() -> u32 {
    30
}

fn default_transfer_window() -> u32 {
    15
}

fn default_scale() -> u8 {
    2
}

fn random_rule_salt() -> [u8; 32] {
    let mut salt = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut salt);
    salt
}

pub fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RetentionPolicy {
    #[default]
    None,
    History,
    Transactions,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TransactionRetention {
    #[default]
    #[serde(rename = "1y")]
    OneYear,
    #[serde(rename = "2y")]
    TwoYears,
    Forever,
}

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
pub enum TxDirection {
    In,
    Out,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TxClassification {
    #[default]
    Normal,
    Transfer,
    PossibleTransfer,
    DebtPayment,
    RepaymentReceived,
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
    UnmatchedNeedsReview,
    Ignored,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MoneySourceConfig {
    pub id: String,
    #[serde(rename = "type")]
    pub source_type: MoneySourceType,
    pub rule_template: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppNotificationPolicy {
    #[serde(default)]
    pub retention: RetentionPolicy,
    #[serde(default = "default_retention_days")]
    pub retention_days: u32,
    #[serde(default)]
    pub money_sources: Vec<MoneySourceConfig>,
    #[serde(default)]
    pub sensitive: bool,
}

impl Default for AppNotificationPolicy {
    fn default() -> Self {
        Self {
            retention: RetentionPolicy::None,
            retention_days: default_retention_days(),
            money_sources: Vec::new(),
            sensitive: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NotificationStorageConfig {
    #[serde(default = "default_version_v2")]
    pub version: u32,
    #[serde(default)]
    pub history_enabled: bool,
    #[serde(default)]
    pub background_capture: bool,
    #[serde(default)]
    pub transaction_retention: TransactionRetention,
    #[serde(default = "default_transfer_window")]
    pub transfer_match_window_minutes: u32,
    #[serde(default)]
    pub large_income_threshold_by_currency: HashMap<String, i64>,
    #[serde(default)]
    pub per_app: HashMap<String, AppNotificationPolicy>,
}

impl Default for NotificationStorageConfig {
    fn default() -> Self {
        Self {
            version: 2,
            history_enabled: false,
            background_capture: false,
            transaction_retention: TransactionRetention::OneYear,
            transfer_match_window_minutes: default_transfer_window(),
            large_income_threshold_by_currency: HashMap::new(),
            per_app: HashMap::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StoredNotification {
    pub dedupe_id: String,
    pub hw_serial: String,
    pub key: String,
    pub package: String,
    pub app_label: String,
    pub title: String,
    pub text: String,
    pub posted_unix: i64,
    pub captured_unix: i64,
    #[serde(default)]
    pub hidden_local: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StoredTransaction {
    pub id: String,
    pub dedupe_id: String,
    pub hw_serial: String,
    pub package: String,
    #[serde(default)]
    pub source_id: String,
    pub app_label: String,
    pub source_type: MoneySourceType,
    #[serde(default)]
    pub sub_account: Option<String>,
    pub direction: TxDirection,
    pub amount_minor: i64,
    #[serde(default = "default_scale")]
    pub scale: u8,
    pub currency: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub classification: TxClassification,
    #[serde(default)]
    pub manual_classification: Option<TxClassification>,
    #[serde(default)]
    pub manual_category: Option<String>,
    #[serde(default)]
    pub transfer_pair_id: Option<String>,
    #[serde(default)]
    pub linked_liability_event_id: Option<String>,
    #[serde(default)]
    pub linked_liability_source_id: Option<String>,
    #[serde(default)]
    pub linked_sub_account: Option<String>,
    #[serde(default)]
    pub transfer_hint: Option<TransferHint>,
    #[serde(default)]
    pub similarity_hash: Option<String>,
    #[serde(default)]
    pub statement_due_minor: Option<i64>,
    #[serde(default)]
    pub statement_due_unix: Option<i64>,
    pub timestamp_ms: i64,
    #[serde(default)]
    pub rule_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LiabilityEvent {
    pub id: String,
    pub dedupe_id: String,
    pub hw_serial: String,
    pub package: String,
    pub source_id: String,
    pub app_label: String,
    pub source_type: MoneySourceType,
    pub event_type: LiabilityEventType,
    #[serde(default)]
    pub sub_account: Option<String>,
    pub status: LiabilityEventStatus,
    #[serde(default)]
    pub linked_tx_id: Option<String>,
    #[serde(default)]
    pub resolved_amount_minor: Option<i64>,
    #[serde(default)]
    pub resolved_currency: Option<String>,
    #[serde(default)]
    pub scale: Option<u8>,
    #[serde(default)]
    pub manual_locked: bool,
    #[serde(default)]
    pub duplicate_of: Option<String>,
    pub timestamp_ms: i64,
    #[serde(default)]
    pub rule_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CorrectionRule {
    pub id: String,
    pub package: String,
    pub direction: TxDirection,
    pub similarity_hash: String,
    #[serde(default)]
    pub classification: Option<TxClassification>,
    #[serde(default)]
    pub category: Option<String>,
    pub created_unix: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryVault {
    #[serde(default = "default_vault_version")]
    pub version: u32,
    #[serde(default)]
    pub entries: Vec<StoredNotification>,
}

impl Default for HistoryVault {
    fn default() -> Self {
        Self {
            version: VAULT_VERSION,
            entries: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TransactionsVault {
    #[serde(default = "default_vault_version")]
    pub version: u32,
    #[serde(default = "random_rule_salt")]
    pub rule_salt: [u8; 32],
    #[serde(default)]
    pub transactions: Vec<StoredTransaction>,
    #[serde(default)]
    pub liability_events: Vec<LiabilityEvent>,
    #[serde(default)]
    pub sub_account_nicknames: HashMap<String, String>,
    #[serde(default)]
    pub correction_rules: Vec<CorrectionRule>,
}

impl Default for TransactionsVault {
    fn default() -> Self {
        Self {
            version: VAULT_VERSION,
            rule_salt: random_rule_salt(),
            transactions: Vec::new(),
            liability_events: Vec::new(),
            sub_account_nicknames: HashMap::new(),
            correction_rules: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeleteResult {
    pub deleted_notifications: usize,
    pub deleted_transactions: usize,
    pub deleted_liability_events: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    pub history_enabled: bool,
    pub vault_error: Option<String>,
    pub saved_hw_serials: Vec<String>,
}

#[cfg(windows)]
pub fn dpapi_protect_with_entropy(plaintext: &[u8], entropy: &[u8]) -> Result<Vec<u8>, String> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let mut input_buf = plaintext.to_vec();
    let in_blob = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(input_buf.len())
            .map_err(|_| "Vault payload too large".to_string())?,
        pbData: if input_buf.is_empty() {
            std::ptr::null_mut()
        } else {
            input_buf.as_mut_ptr()
        },
    };
    let mut entropy_buf = entropy.to_vec();
    let entropy_blob = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(entropy_buf.len()).map_err(|_| "Entropy too large".to_string())?,
        pbData: if entropy_buf.is_empty() {
            std::ptr::null_mut()
        } else {
            entropy_buf.as_mut_ptr()
        },
    };
    let mut out_blob = CRYPT_INTEGER_BLOB::default();

    let entropy_opt = (!entropy.is_empty()).then_some(&entropy_blob as *const CRYPT_INTEGER_BLOB);

    // SAFETY: Blobs point to live stack/vec buffers for the duration of CryptProtectData,
    // and out_blob.pbData is released via LocalFree before returning.
    unsafe {
        CryptProtectData(
            &in_blob,
            None,
            entropy_opt,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out_blob,
        )
        .map_err(|e| format!("DPAPI encryption failed: {e}"))?;

        let slice = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize);
        let encrypted = slice.to_vec();
        let _ = LocalFree(Some(HLOCAL(out_blob.pbData.cast())));
        Ok(encrypted)
    }
}

#[cfg(windows)]
pub fn dpapi_unprotect_with_entropy(ciphertext: &[u8], entropy: &[u8]) -> Result<Vec<u8>, String> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    if ciphertext.is_empty() {
        return Err(format!(
            "{VAULT_DECRYPT_FAILED_PREFIX} Unable to decrypt local vault for this Windows user (empty file)"
        ));
    }

    let mut input_buf = ciphertext.to_vec();
    let in_blob = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(input_buf.len())
            .map_err(|_| "Vault payload too large".to_string())?,
        pbData: input_buf.as_mut_ptr(),
    };
    let mut entropy_buf = entropy.to_vec();
    let entropy_blob = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(entropy_buf.len()).map_err(|_| "Entropy too large".to_string())?,
        pbData: if entropy_buf.is_empty() {
            std::ptr::null_mut()
        } else {
            entropy_buf.as_mut_ptr()
        },
    };
    let mut out_blob = CRYPT_INTEGER_BLOB::default();

    let entropy_opt = (!entropy.is_empty()).then_some(&entropy_blob as *const CRYPT_INTEGER_BLOB);

    // SAFETY: Blobs point to live vec buffers for the duration of CryptUnprotectData,
    // and out_blob.pbData is released via LocalFree before returning.
    unsafe {
        CryptUnprotectData(
            &in_blob,
            None,
            entropy_opt,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out_blob,
        )
        .map_err(|e| {
            format!(
                "{VAULT_DECRYPT_FAILED_PREFIX} Unable to decrypt local vault for this Windows user ({e})"
            )
        })?;

        let slice = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize);
        let decrypted = slice.to_vec();
        let _ = LocalFree(Some(HLOCAL(out_blob.pbData.cast())));
        Ok(decrypted)
    }
}

#[cfg(not(windows))]
pub fn dpapi_protect_with_entropy(_plaintext: &[u8], _entropy: &[u8]) -> Result<Vec<u8>, String> {
    Err("Encrypted local vault is only supported on Windows".to_string())
}

#[cfg(not(windows))]
pub fn dpapi_unprotect_with_entropy(
    _ciphertext: &[u8],
    _entropy: &[u8],
) -> Result<Vec<u8>, String> {
    Err("Encrypted local vault is only supported on Windows".to_string())
}

pub fn dpapi_protect(plaintext: &[u8]) -> Result<Vec<u8>, String> {
    dpapi_protect_with_entropy(plaintext, VAULT_ENTROPY)
}

pub fn dpapi_unprotect(ciphertext: &[u8]) -> Result<Vec<u8>, String> {
    dpapi_unprotect_with_entropy(ciphertext, VAULT_ENTROPY)
}

pub fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Invalid vault file path".to_string())?;
    fs::create_dir_all(parent).map_err(|e| format!("Failed to create vault directory: {e}"))?;

    let seq = ATOMIC_TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("vault.dpapi");
    let tmp_path = parent.join(format!("{file_name}.tmp.{}.{seq}", std::process::id()));

    let write_res = (|| -> Result<(), String> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
            .map_err(|e| format!("Failed to open temp vault file: {e}"))?;
        file.write_all(bytes)
            .map_err(|e| format!("Failed to write temp vault file: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("Failed to sync temp vault file: {e}"))?;
        drop(file);
        fs::rename(&tmp_path, path)
            .map_err(|e| format!("Failed to finalize atomic vault write: {e}"))?;
        Ok(())
    })();

    if write_res.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    write_res
}

fn encrypt_and_write<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let json = serde_json::to_vec(value).map_err(|e| format!("Failed to serialize vault: {e}"))?;
    let encrypted = dpapi_protect(&json)?;
    atomic_write_bytes(path, &encrypted)
}

fn read_and_decrypt<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let bytes = fs::read(path).map_err(|e| format!("Failed to read vault file: {e}"))?;
    let plaintext = dpapi_unprotect(&bytes)?;
    serde_json::from_slice(&plaintext).map_err(|e| {
        format!("{VAULT_DECRYPT_FAILED_PREFIX} Unable to parse decrypted vault JSON ({e})")
    })
}

pub fn make_dedupe_id(hw_serial: &str, key: &str, posted_unix: i64) -> String {
    format!("{hw_serial}::{key}::{posted_unix}")
}

fn normalize_retention_days(days: u32) -> i64 {
    match days {
        7 | 30 | 90 => days as i64,
        _ => 30,
    }
}

pub fn prune_expired_history(
    vault: &mut HistoryVault,
    per_app: &HashMap<String, AppNotificationPolicy>,
    now_unix: i64,
) -> bool {
    let before = vault.entries.len();
    vault.entries.retain(|entry| {
        let Some(policy) = per_app.get(&entry.package) else {
            return true;
        };
        match policy.retention {
            RetentionPolicy::History => {
                let days = normalize_retention_days(policy.retention_days);
                let cutoff = now_unix.saturating_sub(days.saturating_mul(SECS_PER_DAY));
                let ref_ts = if entry.posted_unix > 0 {
                    entry.posted_unix
                } else {
                    entry.captured_unix
                };
                ref_ts >= cutoff
            }
            RetentionPolicy::Transactions => false,
            RetentionPolicy::None => true,
        }
    });
    vault.entries.len() != before
}

pub fn enforce_history_capacity(vault: &mut HistoryVault) -> bool {
    if vault.entries.len() <= HISTORY_CAPACITY {
        return false;
    }
    vault.entries.sort_by(|a, b| {
        b.posted_unix
            .cmp(&a.posted_unix)
            .then_with(|| b.captured_unix.cmp(&a.captured_unix))
            .then_with(|| a.dedupe_id.cmp(&b.dedupe_id))
    });
    vault.entries.truncate(HISTORY_CAPACITY);
    true
}

pub fn prune_expired_transactions(
    vault: &mut TransactionsVault,
    retention: TransactionRetention,
    now_ms: i64,
) -> bool {
    let days = match retention {
        TransactionRetention::OneYear => 365_i64,
        TransactionRetention::TwoYears => 730_i64,
        TransactionRetention::Forever => return false,
    };
    let cutoff_ms = now_ms.saturating_sub(days.saturating_mul(MS_PER_DAY));
    let tx_before = vault.transactions.len();
    let ev_before = vault.liability_events.len();

    vault.transactions.retain(|tx| tx.timestamp_ms >= cutoff_ms);
    vault
        .liability_events
        .retain(|ev| ev.timestamp_ms >= cutoff_ms);

    vault.transactions.len() != tx_before || vault.liability_events.len() != ev_before
}

pub fn enforce_transactions_capacity(vault: &mut TransactionsVault) -> bool {
    if vault.transactions.len() <= TRANSACTIONS_CAPACITY {
        return false;
    }
    // Keep newest transactions; if over cap, prefer evicting non-manually-locked oldest rows.
    vault.transactions.sort_by(|a, b| {
        let a_locked = a.manual_classification.is_some() || a.manual_category.is_some();
        let b_locked = b.manual_classification.is_some() || b.manual_category.is_some();
        b_locked
            .cmp(&a_locked)
            .then_with(|| b.timestamp_ms.cmp(&a.timestamp_ms))
            .then_with(|| a.id.cmp(&b.id))
    });
    // Wait: sorting by locked first could keep an ancient locked tx over a recent unlocked one only when
    // strictly above 50k rows, matching SPEC-33 §8.3 ("oldest non-rule-protected transaction evicted first").
    vault.transactions.truncate(TRANSACTIONS_CAPACITY);
    true
}

#[derive(Default)]
pub struct VaultStore {
    base_dir: Option<PathBuf>,
    loaded: bool,
    vault_error: Option<String>,
    config: NotificationStorageConfig,
    history: HistoryVault,
    transactions: TransactionsVault,
    seen_dedupe_ids: HashSet<String>,
}

impl VaultStore {
    pub fn new_with_dir(base_dir: PathBuf) -> Self {
        Self {
            base_dir: Some(base_dir),
            ..Default::default()
        }
    }

    pub fn base_dir(&self) -> Option<&PathBuf> {
        self.base_dir.as_ref()
    }

    pub fn transactions_vault_mut(&mut self) -> &mut TransactionsVault {
        &mut self.transactions
    }

    pub fn save_transactions_vault(&mut self) -> Result<(), String> {
        let tx_path = self.transactions_path()?;
        encrypt_and_write(&tx_path, &self.transactions)?;
        self.rebuild_seen_dedupe_ids();
        Ok(())
    }

    pub fn set_base_dir_if_unset(&mut self, dir: PathBuf) {
        if self.base_dir.is_none() {
            self.base_dir = Some(dir);
        }
    }

    pub fn config(&self) -> &NotificationStorageConfig {
        &self.config
    }

    pub fn vault_error(&self) -> Option<&str> {
        self.vault_error.as_deref()
    }

    pub fn history_vault(&self) -> &HistoryVault {
        &self.history
    }

    pub fn transactions_vault(&self) -> &TransactionsVault {
        &self.transactions
    }

    pub fn history_path(&self) -> Result<PathBuf, String> {
        self.base_dir
            .as_ref()
            .map(|d| d.join(HISTORY_VAULT_FILENAME))
            .ok_or_else(|| "Vault base directory is not initialized".to_string())
    }

    pub fn transactions_path(&self) -> Result<PathBuf, String> {
        self.base_dir
            .as_ref()
            .map(|d| d.join(TRANSACTIONS_VAULT_FILENAME))
            .ok_or_else(|| "Vault base directory is not initialized".to_string())
    }

    fn rebuild_seen_dedupe_ids(&mut self) {
        self.seen_dedupe_ids.clear();
        for entry in &self.history.entries {
            self.seen_dedupe_ids.insert(entry.dedupe_id.clone());
        }
        for tx in &self.transactions.transactions {
            self.seen_dedupe_ids.insert(tx.dedupe_id.clone());
        }
        for ev in &self.transactions.liability_events {
            self.seen_dedupe_ids.insert(ev.dedupe_id.clone());
        }
    }

    pub fn ensure_loaded(&mut self) -> Result<(), String> {
        if let Some(ref err) = self.vault_error {
            return Err(err.clone());
        }
        if self.loaded {
            return Ok(());
        }

        let hist_path = self.history_path()?;
        let tx_path = self.transactions_path()?;

        let loaded_history = if hist_path.exists() {
            match read_and_decrypt::<HistoryVault>(&hist_path) {
                Ok(v) => v,
                Err(e) => {
                    self.vault_error = Some(e.clone());
                    return Err(e);
                }
            }
        } else {
            HistoryVault::default()
        };

        let loaded_transactions = if tx_path.exists() {
            match read_and_decrypt::<TransactionsVault>(&tx_path) {
                Ok(v) => v,
                Err(e) => {
                    self.vault_error = Some(e.clone());
                    return Err(e);
                }
            }
        } else {
            TransactionsVault::default()
        };

        self.history = loaded_history;
        self.transactions = loaded_transactions;
        self.rebuild_seen_dedupe_ids();
        self.loaded = true;
        Ok(())
    }

    pub fn saved_hw_serials(&self) -> Vec<String> {
        let mut set: HashSet<String> = HashSet::new();
        for entry in &self.history.entries {
            if !entry.hw_serial.is_empty() {
                set.insert(entry.hw_serial.clone());
            }
        }
        for tx in &self.transactions.transactions {
            if !tx.hw_serial.is_empty() {
                set.insert(tx.hw_serial.clone());
            }
        }
        let mut out: Vec<String> = set.into_iter().collect();
        out.sort();
        out
    }

    pub fn saved_packages(&self) -> Vec<String> {
        let mut set: HashSet<String> = HashSet::new();
        for entry in &self.history.entries {
            if !entry.package.is_empty() {
                set.insert(entry.package.clone());
            }
        }
        for tx in &self.transactions.transactions {
            if !tx.package.is_empty() {
                set.insert(tx.package.clone());
            }
        }
        for ev in &self.transactions.liability_events {
            if !ev.package.is_empty() {
                set.insert(ev.package.clone());
            }
        }
        for rule in &self.transactions.correction_rules {
            if !rule.package.is_empty() {
                set.insert(rule.package.clone());
            }
        }
        let mut out: Vec<String> = set.into_iter().collect();
        out.sort();
        out
    }

    pub fn status(&self) -> VaultStatus {
        VaultStatus {
            history_enabled: self.config.history_enabled,
            vault_error: self.vault_error.clone(),
            saved_hw_serials: if self.loaded {
                self.saved_hw_serials()
            } else {
                Vec::new()
            },
        }
    }

    pub fn apply_config(
        &mut self,
        config: NotificationStorageConfig,
        now_unix: i64,
    ) -> Result<VaultStatus, String> {
        let prev_window = self.config.transfer_match_window_minutes;
        self.config = config;
        if !self.config.history_enabled {
            // Master opt-in off: write nothing to disk and preserve existing vaults if any.
            return Ok(self.status());
        }

        self.ensure_loaded()?;

        let tx_path = self.transactions_path()?;
        if !tx_path.exists() {
            encrypt_and_write(&tx_path, &self.transactions)?;
        }

        let mut history_dirty =
            prune_expired_history(&mut self.history, &self.config.per_app, now_unix);
        history_dirty |= enforce_history_capacity(&mut self.history);

        let now_ms = now_unix.saturating_mul(1000);
        let mut tx_dirty = prune_expired_transactions(
            &mut self.transactions,
            self.config.transaction_retention,
            now_ms,
        );
        if prev_window != self.config.transfer_match_window_minutes {
            tx_dirty |= crate::phone_hub::money::recompute_matches(
                &mut self.transactions,
                self.config.transfer_match_window_minutes,
            );
        }
        tx_dirty |= enforce_transactions_capacity(&mut self.transactions);

        if history_dirty {
            let hist_path = self.history_path()?;
            encrypt_and_write(&hist_path, &self.history)?;
            self.rebuild_seen_dedupe_ids();
        }
        if tx_dirty {
            encrypt_and_write(&tx_path, &self.transactions)?;
            self.rebuild_seen_dedupe_ids();
        }

        Ok(self.status())
    }

    /// Ingest a batch of polled notifications.
    /// Returns `Ok(true)` if any encrypted vault file was written to disk, `Ok(false)` on idle/no-op.
    pub fn ingest_notifications(
        &mut self,
        hw_serial: &str,
        items: &[PhoneHubNotification],
        now_unix: i64,
    ) -> Result<bool, String> {
        if !self.config.history_enabled {
            return Ok(false);
        }
        self.ensure_loaded()?;

        let has_any_enabled_source = self.config.per_app.values().any(|p| {
            p.retention != RetentionPolicy::None && p.money_sources.iter().any(|s| s.enabled)
        });
        let loaded_rules = if has_any_enabled_source {
            Some(crate::phone_hub::money::load_rules_for_dir(
                self.base_dir.as_deref(),
            ))
        } else {
            None
        };

        let mut history_dirty = false;
        let mut tx_dirty = false;
        for item in items {
            let dedupe_id = make_dedupe_id(hw_serial, &item.key, item.posted_unix);
            if self.seen_dedupe_ids.contains(&dedupe_id) {
                continue;
            }
            let Some(policy) = self.config.per_app.get(&item.package) else {
                continue;
            };
            if policy.retention == RetentionPolicy::None {
                continue;
            }
            if policy.retention == RetentionPolicy::History {
                self.history.entries.push(StoredNotification {
                    dedupe_id: dedupe_id.clone(),
                    hw_serial: hw_serial.to_string(),
                    key: item.key.clone(),
                    package: item.package.clone(),
                    app_label: item.app_label.clone(),
                    title: item.title.clone(),
                    text: item.text.clone(),
                    posted_unix: item.posted_unix,
                    captured_unix: now_unix,
                    hidden_local: false,
                });
                history_dirty = true;
            }

            if policy.money_sources.iter().any(|s| s.enabled) {
                if let Some(ref rules) = loaded_rules {
                    match crate::phone_hub::money::parse_notification_for_sources(
                        rules,
                        hw_serial,
                        item,
                        policy,
                        now_unix,
                        &self.transactions.rule_salt,
                    ) {
                        Some(crate::phone_hub::money::ParsedMoneyOutcome::Transaction(mut tx)) => {
                            crate::phone_hub::money::apply_matching_correction_rule(
                                &mut tx,
                                &self.transactions.correction_rules,
                            );
                            if !self
                                .transactions
                                .transactions
                                .iter()
                                .any(|existing| existing.id == tx.id)
                            {
                                self.transactions.transactions.push(tx);
                                tx_dirty = true;
                            }
                        }
                        Some(crate::phone_hub::money::ParsedMoneyOutcome::LiabilityEvent(ev))
                            if !self
                                .transactions
                                .liability_events
                                .iter()
                                .any(|existing| existing.id == ev.id) =>
                        {
                            self.transactions.liability_events.push(ev);
                            tx_dirty = true;
                        }
                        _ => {}
                    }
                }
            }

            self.seen_dedupe_ids.insert(dedupe_id);
        }

        if !history_dirty && !tx_dirty {
            return Ok(false);
        }

        if history_dirty {
            prune_expired_history(&mut self.history, &self.config.per_app, now_unix);
            enforce_history_capacity(&mut self.history);
            let hist_path = self.history_path()?;
            encrypt_and_write(&hist_path, &self.history)?;
        }

        if tx_dirty {
            crate::phone_hub::money::recompute_matches(
                &mut self.transactions,
                self.config.transfer_match_window_minutes,
            );
            prune_expired_transactions(
                &mut self.transactions,
                self.config.transaction_retention,
                now_unix.saturating_mul(1000),
            );
            enforce_transactions_capacity(&mut self.transactions);
            let tx_path = self.transactions_path()?;
            encrypt_and_write(&tx_path, &self.transactions)?;
        } else {
            let tx_pruned = prune_expired_transactions(
                &mut self.transactions,
                self.config.transaction_retention,
                now_unix.saturating_mul(1000),
            );
            if tx_pruned {
                let tx_path = self.transactions_path()?;
                encrypt_and_write(&tx_path, &self.transactions)?;
            }
        }

        Ok(true)
    }

    pub fn get_history(
        &mut self,
        hw_serial: &str,
        limit: usize,
        before_unix: Option<i64>,
    ) -> Result<Vec<StoredNotification>, String> {
        if !self.config.history_enabled {
            return Ok(Vec::new());
        }
        self.ensure_loaded()?;

        let mut items: Vec<StoredNotification> = self
            .history
            .entries
            .iter()
            .filter(|n| {
                n.hw_serial == hw_serial && before_unix.is_none_or(|before| n.posted_unix < before)
            })
            .cloned()
            .collect();

        items.sort_by(|a, b| {
            b.posted_unix
                .cmp(&a.posted_unix)
                .then_with(|| b.captured_unix.cmp(&a.captured_unix))
                .then_with(|| a.dedupe_id.cmp(&b.dedupe_id))
        });
        items.truncate(limit.clamp(1, HISTORY_CAPACITY));
        Ok(items)
    }

    pub fn set_notification_hidden(&mut self, dedupe_id: &str, hidden: bool) -> Result<(), String> {
        if !self.config.history_enabled {
            return Ok(());
        }
        self.ensure_loaded()?;

        let Some(entry) = self
            .history
            .entries
            .iter_mut()
            .find(|n| n.dedupe_id == dedupe_id)
        else {
            return Ok(());
        };
        if entry.hidden_local == hidden {
            return Ok(());
        }
        entry.hidden_local = hidden;
        let hist_path = self.history_path()?;
        encrypt_and_write(&hist_path, &self.history)
    }

    pub fn delete_for_package(
        &mut self,
        package: &str,
        hw_serial: Option<&str>,
        delete_rules: bool,
    ) -> Result<DeleteResult, String> {
        let hist_path = self.history_path()?;
        let tx_path = self.transactions_path()?;
        if !self.loaded && !hist_path.exists() && !tx_path.exists() {
            return Ok(DeleteResult::default());
        }
        self.ensure_loaded()?;

        let matches_target = |pkg: &str, serial: &str| {
            pkg == package && hw_serial.is_none_or(|target| serial == target)
        };

        let hist_before = self.history.entries.len();
        self.history
            .entries
            .retain(|n| !matches_target(&n.package, &n.hw_serial));
        let deleted_notifications = hist_before - self.history.entries.len();

        let tx_before = self.transactions.transactions.len();
        self.transactions
            .transactions
            .retain(|tx| !matches_target(&tx.package, &tx.hw_serial));
        let deleted_transactions = tx_before - self.transactions.transactions.len();

        let ev_before = self.transactions.liability_events.len();
        self.transactions
            .liability_events
            .retain(|ev| !matches_target(&ev.package, &ev.hw_serial));
        let deleted_liability_events = ev_before - self.transactions.liability_events.len();

        let mut rules_removed = false;
        if delete_rules {
            let rules_before = self.transactions.correction_rules.len();
            self.transactions
                .correction_rules
                .retain(|r| r.package != package);
            rules_removed = self.transactions.correction_rules.len() != rules_before;
        }

        if deleted_notifications > 0 && hist_path.exists() {
            encrypt_and_write(&hist_path, &self.history)?;
        }
        if (deleted_transactions > 0 || deleted_liability_events > 0 || rules_removed)
            && tx_path.exists()
        {
            encrypt_and_write(&tx_path, &self.transactions)?;
        }

        self.rebuild_seen_dedupe_ids();

        Ok(DeleteResult {
            deleted_notifications,
            deleted_transactions,
            deleted_liability_events,
        })
    }

    pub fn delete_all(&mut self) -> Result<DeleteResult, String> {
        let result = if self.loaded {
            DeleteResult {
                deleted_notifications: self.history.entries.len(),
                deleted_transactions: self.transactions.transactions.len(),
                deleted_liability_events: self.transactions.liability_events.len(),
            }
        } else {
            DeleteResult::default()
        };

        if let Ok(hist_path) = self.history_path() {
            if hist_path.exists() {
                fs::remove_file(&hist_path)
                    .map_err(|e| format!("Failed to delete history vault: {e}"))?;
            }
        }
        if let Ok(tx_path) = self.transactions_path() {
            if tx_path.exists() {
                fs::remove_file(&tx_path)
                    .map_err(|e| format!("Failed to delete transactions vault: {e}"))?;
            }
        }

        self.history = HistoryVault::default();
        self.transactions = TransactionsVault::default();
        self.seen_dedupe_ids.clear();
        self.vault_error = None;
        self.loaded = true;

        if self.config.history_enabled {
            if let Ok(tx_path) = self.transactions_path() {
                encrypt_and_write(&tx_path, &self.transactions)?;
            }
        }

        Ok(result)
    }
}

pub(crate) fn ensure_vault_dir(app: &AppHandle, store: &mut VaultStore) -> Result<(), String> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("Failed to resolve local app data dir: {e}"))?;
    store.set_base_dir_if_unset(dir);
    Ok(())
}

pub(crate) fn ingest_notifications_into_vault(
    app: &AppHandle,
    state: &PhoneHubState,
    hw_serial: &str,
    items: &[PhoneHubNotification],
) {
    let Ok(mut store) = state.notification_vault.lock() else {
        return;
    };
    if !store.config().history_enabled {
        return;
    }
    if ensure_vault_dir(app, &mut store).is_err() {
        return;
    }
    match store.ingest_notifications(hw_serial, items, now_unix_secs()) {
        Ok(true) => {
            #[cfg(debug_assertions)]
            eprintln!(
                "[phone_hub::vault] persisted batch hw_serial={} count={}",
                hw_serial,
                items.len()
            );
        }
        Ok(false) => {}
        Err(_e) => {
            #[cfg(debug_assertions)]
            eprintln!("[phone_hub::vault] skipped persist due to vault error");
        }
    }
}

#[tauri::command]
pub fn phone_set_notification_storage_config(
    app: AppHandle,
    state: State<'_, PhoneHubState>,
    config: NotificationStorageConfig,
) -> Result<VaultStatus, String> {
    let mut store = state
        .notification_vault
        .lock()
        .map_err(crate::state::poison_error)?;
    ensure_vault_dir(&app, &mut store)?;
    store.apply_config(config, now_unix_secs())
}

#[tauri::command]
pub fn phone_get_vault_status(
    app: AppHandle,
    state: State<'_, PhoneHubState>,
) -> Result<VaultStatus, String> {
    let mut store = state
        .notification_vault
        .lock()
        .map_err(crate::state::poison_error)?;
    ensure_vault_dir(&app, &mut store)?;
    if store.config().history_enabled {
        let _ = store.ensure_loaded();
    }
    Ok(store.status())
}

#[tauri::command]
pub fn phone_get_saved_notification_devices(
    app: AppHandle,
    state: State<'_, PhoneHubState>,
) -> Result<Vec<String>, String> {
    let mut store = state
        .notification_vault
        .lock()
        .map_err(crate::state::poison_error)?;
    if !store.config().history_enabled {
        return Ok(Vec::new());
    }
    ensure_vault_dir(&app, &mut store)?;
    store.ensure_loaded()?;
    Ok(store.saved_hw_serials())
}

#[tauri::command]
pub fn phone_get_saved_notification_packages(
    app: AppHandle,
    state: State<'_, PhoneHubState>,
) -> Result<Vec<String>, String> {
    let mut store = state
        .notification_vault
        .lock()
        .map_err(crate::state::poison_error)?;
    ensure_vault_dir(&app, &mut store)?;
    if !store.loaded {
        let hist_exists = store.history_path().map(|p| p.exists()).unwrap_or(false);
        let tx_exists = store
            .transactions_path()
            .map(|p| p.exists())
            .unwrap_or(false);
        if !hist_exists && !tx_exists {
            return Ok(Vec::new());
        }
        store.ensure_loaded()?;
    }
    Ok(store.saved_packages())
}

#[tauri::command]
pub fn phone_get_notification_history(
    app: AppHandle,
    state: State<'_, PhoneHubState>,
    hw_serial: String,
    limit: Option<usize>,
    before_unix: Option<i64>,
) -> Result<Vec<StoredNotification>, String> {
    let mut store = state
        .notification_vault
        .lock()
        .map_err(crate::state::poison_error)?;
    ensure_vault_dir(&app, &mut store)?;
    store.get_history(&hw_serial, limit.unwrap_or(200), before_unix)
}

#[tauri::command]
pub fn phone_set_notification_hidden(
    app: AppHandle,
    state: State<'_, PhoneHubState>,
    dedupe_id: String,
    hidden: bool,
) -> Result<(), String> {
    let mut store = state
        .notification_vault
        .lock()
        .map_err(crate::state::poison_error)?;
    ensure_vault_dir(&app, &mut store)?;
    store.set_notification_hidden(&dedupe_id, hidden)
}

#[tauri::command]
pub fn phone_delete_app_notification_data(
    app: AppHandle,
    state: State<'_, PhoneHubState>,
    package: String,
    hw_serial: Option<String>,
    delete_rules: Option<bool>,
) -> Result<DeleteResult, String> {
    let mut store = state
        .notification_vault
        .lock()
        .map_err(crate::state::poison_error)?;
    ensure_vault_dir(&app, &mut store)?;
    store.delete_for_package(
        &package,
        hw_serial.as_deref(),
        delete_rules.unwrap_or(false),
    )
}

#[tauri::command]
pub fn phone_delete_all_notification_data(
    app: AppHandle,
    state: State<'_, PhoneHubState>,
) -> Result<DeleteResult, String> {
    let mut store = state
        .notification_vault
        .lock()
        .map_err(crate::state::poison_error)?;
    ensure_vault_dir(&app, &mut store)?;
    store.delete_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn temp_test_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "anyleap_vault_test_{tag}_{}_{}",
            std::process::id(),
            ATOMIC_TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_notif(
        pkg: &str,
        key: &str,
        title: &str,
        text: &str,
        posted_unix: i64,
    ) -> PhoneHubNotification {
        PhoneHubNotification {
            key: key.to_string(),
            package: pkg.to_string(),
            app_label: "Example App".to_string(),
            title: title.to_string(),
            text: text.to_string(),
            posted_unix,
        }
    }

    fn history_policy(days: u32) -> AppNotificationPolicy {
        AppNotificationPolicy {
            retention: RetentionPolicy::History,
            retention_days: days,
            money_sources: Vec::new(),
            sensitive: false,
        }
    }

    fn transactions_policy() -> AppNotificationPolicy {
        AppNotificationPolicy {
            retention: RetentionPolicy::Transactions,
            retention_days: 30,
            money_sources: vec![MoneySourceConfig {
                id: "src_1".to_string(),
                source_type: MoneySourceType::Bank,
                rule_template: "generic_bank".to_string(),
                enabled: true,
                label: None,
            }],
            sensitive: true,
        }
    }

    fn make_tx(id: &str, pkg: &str, ts_ms: i64) -> StoredTransaction {
        StoredTransaction {
            id: id.to_string(),
            dedupe_id: format!("HW1::{id}::100"),
            hw_serial: "HW1".to_string(),
            package: pkg.to_string(),
            source_id: "src_1".to_string(),
            app_label: "Example Bank".to_string(),
            source_type: MoneySourceType::Bank,
            sub_account: None,
            direction: TxDirection::Out,
            amount_minor: 5_000_000,
            scale: 2,
            currency: "IDR".to_string(),
            category: "Spending".to_string(),
            classification: TxClassification::Normal,
            manual_classification: None,
            manual_category: None,
            transfer_pair_id: None,
            linked_liability_event_id: None,
            linked_liability_source_id: None,
            linked_sub_account: None,
            transfer_hint: None,
            similarity_hash: None,
            statement_due_minor: None,
            statement_due_unix: None,
            timestamp_ms: ts_ms,
            rule_id: "r1".to_string(),
        }
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_round_trip_tamper_and_wrong_entropy() {
        let secret = br#"{"hello":"vault","amount":12345}"#;
        let encrypted = dpapi_protect(secret).expect("dpapi_protect should succeed");
        assert_ne!(encrypted.as_slice(), secret);

        let decrypted = dpapi_unprotect(&encrypted).expect("dpapi_unprotect should succeed");
        assert_eq!(decrypted, secret);

        // Tamper detection
        let mut tampered = encrypted.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 0xFF;
        let tamper_err = dpapi_unprotect(&tampered).unwrap_err();
        assert!(tamper_err.starts_with(VAULT_DECRYPT_FAILED_PREFIX));

        // Wrong entropy fails
        let wrong_entropy_err =
            dpapi_unprotect_with_entropy(&encrypted, b"Wrong.Entropy.v1").unwrap_err();
        assert!(wrong_entropy_err.starts_with(VAULT_DECRYPT_FAILED_PREFIX));
    }

    #[test]
    fn atomic_write_creates_target_without_leftover_temp_files() {
        let dir = temp_test_dir("atomic");
        let target = dir.join("sample.v1.dpapi");
        atomic_write_bytes(&target, b"payload-1").unwrap();
        atomic_write_bytes(&target, b"payload-2").unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"payload-2");
        let entries: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(entries, vec!["sample.v1.dpapi".to_string()]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn master_switch_off_writes_nothing_to_disk() {
        let dir = temp_test_dir("master_off");
        let mut store = VaultStore::new_with_dir(dir.clone());
        let mut per_app = HashMap::new();
        per_app.insert("com.example.bank".to_string(), history_policy(30));
        let cfg = NotificationStorageConfig {
            history_enabled: false,
            per_app,
            ..Default::default()
        };

        store.apply_config(cfg, 1_700_000_000).unwrap();
        let item = make_notif("com.example.bank", "k1", "Title", "Body", 1_700_000_000);
        let wrote = store
            .ingest_notifications("HW1", &[item], 1_700_000_000)
            .unwrap();
        assert!(!wrote);
        assert!(!dir.join(HISTORY_VAULT_FILENAME).exists());
        assert!(!dir.join(TRANSACTIONS_VAULT_FILENAME).exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn dedupe_and_zero_idle_writes_and_post_time_update() {
        let dir = temp_test_dir("dedupe");
        let mut store = VaultStore::new_with_dir(dir.clone());
        let mut per_app = HashMap::new();
        per_app.insert("com.example.chat".to_string(), history_policy(30));
        per_app.insert(
            "com.example.shop".to_string(),
            AppNotificationPolicy::default(),
        );
        let cfg = NotificationStorageConfig {
            history_enabled: true,
            per_app,
            ..Default::default()
        };
        store.apply_config(cfg, 1_700_000_000).unwrap();

        // Transactions shell created once on opt-in
        let tx_path = dir.join(TRANSACTIONS_VAULT_FILENAME);
        assert!(tx_path.exists());

        let n_chat = make_notif("com.example.chat", "k1", "Alex", "Hey", 1_700_000_010);
        let n_shop = make_notif("com.example.shop", "k2", "Promo", "Deal", 1_700_000_010);
        let wrote = store
            .ingest_notifications("HW1", &[n_chat.clone(), n_shop.clone()], 1_700_000_012)
            .unwrap();
        assert!(wrote);
        assert_eq!(store.history_vault().entries.len(), 1);

        let hist_path = dir.join(HISTORY_VAULT_FILENAME);
        let mtime1 = fs::metadata(&hist_path).unwrap().modified().unwrap();
        std::thread::sleep(Duration::from_millis(25));

        // Identical poll -> deduped, zero disk writes
        let wrote_idle = store
            .ingest_notifications("HW1", &[n_chat, n_shop], 1_700_000_022)
            .unwrap();
        assert!(!wrote_idle);
        let mtime2 = fs::metadata(&hist_path).unwrap().modified().unwrap();
        assert_eq!(mtime1, mtime2);

        // Updated postTime on same key -> recorded once
        let n_chat_updated =
            make_notif("com.example.chat", "k1", "Alex", "Hey again", 1_700_000_030);
        let wrote_update = store
            .ingest_notifications("HW1", &[n_chat_updated], 1_700_000_032)
            .unwrap();
        assert!(wrote_update);
        assert_eq!(store.history_vault().entries.len(), 2);

        // Reload from disk in fresh store and verify DPAPI persistence & zero plaintext
        let raw_bytes = fs::read(&hist_path).unwrap();
        let raw_lossy = String::from_utf8_lossy(&raw_bytes);
        assert!(!raw_lossy.contains("Hey again"));
        assert!(!raw_lossy.contains("com.example.chat"));

        let mut reloaded = VaultStore::new_with_dir(dir.clone());
        let mut per_app2 = HashMap::new();
        per_app2.insert("com.example.chat".to_string(), history_policy(30));
        let cfg2 = NotificationStorageConfig {
            history_enabled: true,
            per_app: per_app2,
            ..Default::default()
        };
        reloaded.apply_config(cfg2, 1_700_000_040).unwrap();
        let items = reloaded.get_history("HW1", 50, None).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].text, "Hey again");

        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn transactions_only_stores_zero_raw_text_and_downgrades_preserve_transactions() {
        let dir = temp_test_dir("downgrade");
        let mut store = VaultStore::new_with_dir(dir.clone());
        let now = 1_700_000_000_i64;
        let mut per_app = HashMap::new();
        per_app.insert("com.example.bank".to_string(), history_policy(30));
        per_app.insert("com.example.txonly".to_string(), transactions_policy());
        let mut cfg = NotificationStorageConfig {
            history_enabled: true,
            per_app,
            ..Default::default()
        };
        store.apply_config(cfg.clone(), now).unwrap();

        let bank_notif = make_notif("com.example.bank", "k1", "Debit", "Secret text", now);
        let tx_only_notif = make_notif(
            "com.example.txonly",
            "k2",
            "OTP 123456",
            "Top secret body",
            now,
        );
        store
            .ingest_notifications("HW1", &[bank_notif, tx_only_notif], now)
            .unwrap();
        assert_eq!(store.history_vault().entries.len(), 1);
        assert_eq!(store.history_vault().entries[0].package, "com.example.bank");

        // Seed a derived transaction in TransactionsVault to verify downgrade & pruning independence
        store
            .transactions
            .transactions
            .push(make_tx("tx1", "com.example.bank", now * 1000));
        encrypt_and_write(&dir.join(TRANSACTIONS_VAULT_FILENAME), &store.transactions).unwrap();

        // Downgrade com.example.bank: history -> transactions purges history but keeps transactions
        cfg.per_app
            .insert("com.example.bank".to_string(), transactions_policy());
        store.apply_config(cfg, now + 10).unwrap();
        assert!(store.history_vault().entries.is_empty());
        assert_eq!(store.transactions_vault().transactions.len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn independent_pruning_windows_and_rule_survival() {
        let now_unix = 2_000_000_000_i64;
        let now_ms = now_unix * 1000;
        let mut hist = HistoryVault::default();
        hist.entries.push(StoredNotification {
            dedupe_id: "d1".into(),
            hw_serial: "HW1".into(),
            key: "k1".into(),
            package: "com.example.seven".into(),
            app_label: "Seven".into(),
            title: "Old".into(),
            text: "Old".into(),
            posted_unix: now_unix - 8 * SECS_PER_DAY,
            captured_unix: now_unix - 8 * SECS_PER_DAY,
            hidden_local: false,
        });
        hist.entries.push(StoredNotification {
            dedupe_id: "d2".into(),
            hw_serial: "HW1".into(),
            key: "k2".into(),
            package: "com.example.seven".into(),
            app_label: "Seven".into(),
            title: "Fresh".into(),
            text: "Fresh".into(),
            posted_unix: now_unix - 3 * SECS_PER_DAY,
            captured_unix: now_unix - 3 * SECS_PER_DAY,
            hidden_local: false,
        });

        let mut per_app = HashMap::new();
        per_app.insert("com.example.seven".to_string(), history_policy(7));
        assert!(prune_expired_history(&mut hist, &per_app, now_unix));
        assert_eq!(hist.entries.len(), 1);
        assert_eq!(hist.entries[0].dedupe_id, "d2");

        // Transactions pruning: 1y (365d), 2y (730d), forever; correction_rules always survive
        let mut tx_vault = TransactionsVault::default();
        tx_vault.transactions.push(make_tx(
            "t_400d",
            "com.example.bank",
            now_ms - 400 * MS_PER_DAY,
        ));
        tx_vault.transactions.push(make_tx(
            "t_800d",
            "com.example.bank",
            now_ms - 800 * MS_PER_DAY,
        ));
        tx_vault.transactions.push(make_tx(
            "t_recent",
            "com.example.bank",
            now_ms - 10 * MS_PER_DAY,
        ));
        tx_vault.correction_rules.push(CorrectionRule {
            id: "rule_1".into(),
            package: "com.example.bank".into(),
            direction: TxDirection::Out,
            similarity_hash: "hash1".into(),
            classification: Some(TxClassification::Transfer),
            category: None,
            created_unix: now_unix - 900 * SECS_PER_DAY,
        });

        let mut tx_forever = tx_vault.clone();
        assert!(!prune_expired_transactions(
            &mut tx_forever,
            TransactionRetention::Forever,
            now_ms
        ));
        assert_eq!(tx_forever.transactions.len(), 3);

        let mut tx_2y = tx_vault.clone();
        assert!(prune_expired_transactions(
            &mut tx_2y,
            TransactionRetention::TwoYears,
            now_ms
        ));
        assert_eq!(tx_2y.transactions.len(), 2);
        assert_eq!(tx_2y.correction_rules.len(), 1);

        assert!(prune_expired_transactions(
            &mut tx_vault,
            TransactionRetention::OneYear,
            now_ms
        ));
        assert_eq!(tx_vault.transactions.len(), 1);
        assert_eq!(tx_vault.transactions[0].id, "t_recent");
        assert_eq!(tx_vault.correction_rules.len(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn delete_for_package_and_delete_all_and_fail_closed_recovery() {
        let dir = temp_test_dir("delete");
        let mut store = VaultStore::new_with_dir(dir.clone());
        let now = 1_700_000_000_i64;
        let mut per_app = HashMap::new();
        per_app.insert("com.example.a".to_string(), history_policy(30));
        per_app.insert("com.example.b".to_string(), history_policy(30));
        let cfg = NotificationStorageConfig {
            history_enabled: true,
            per_app,
            ..Default::default()
        };
        store.apply_config(cfg.clone(), now).unwrap();

        let n_a = make_notif("com.example.a", "k1", "A", "hello A", now);
        let n_b = make_notif("com.example.b", "k2", "B", "hello B", now);
        store.ingest_notifications("HW1", &[n_a, n_b], now).unwrap();
        assert_eq!(
            store.saved_packages(),
            vec!["com.example.a".to_string(), "com.example.b".to_string()]
        );

        let del_a = store
            .delete_for_package("com.example.a", None, true)
            .unwrap();
        assert_eq!(del_a.deleted_notifications, 1);
        assert_eq!(store.history_vault().entries.len(), 1);
        assert_eq!(store.history_vault().entries[0].package, "com.example.b");
        assert_eq!(store.saved_packages(), vec!["com.example.b".to_string()]);

        // Tamper history file on disk and confirm a fresh store fails closed without overwriting
        let hist_path = dir.join(HISTORY_VAULT_FILENAME);
        fs::write(&hist_path, b"corrupted-bytes").unwrap();
        let mut broken = VaultStore::new_with_dir(dir.clone());
        let err = broken.apply_config(cfg, now).unwrap_err();
        assert!(err.starts_with(VAULT_DECRYPT_FAILED_PREFIX));

        // Delete all clears corrupted file and reinitializes cleanly
        broken.delete_all().unwrap();
        assert!(!hist_path.exists());
        assert!(broken.vault_error().is_none());

        let _ = fs::remove_dir_all(&dir);
    }
}
