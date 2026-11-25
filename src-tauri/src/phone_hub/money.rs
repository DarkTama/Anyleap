use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;

use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, State};

use crate::phone_files::parse_dumpsys_notifications_rich;
use crate::phone_hub::poller::PhoneHubNotification;
use crate::phone_hub::vault::{
    ensure_vault_dir, make_dedupe_id, AppNotificationPolicy, CorrectionRule, LiabilityEvent,
    LiabilityEventStatus, LiabilityEventType, MoneySourceConfig, MoneySourceType,
    NotificationStorageConfig, RetentionPolicy, StoredTransaction, TransactionsVault, TransferHint,
    TxClassification, TxDirection, VaultStore,
};
use crate::phone_hub::PhoneHubState;

pub const DEFAULT_RULES_JSON: &str = include_str!("../../resources/transaction_rules.default.json");
pub const USER_RULES_FILENAME: &str = "transaction_rules.user.json";
const DEFAULT_CURRENCY_SCALE: u8 = 2;

fn default_true() -> bool {
    true
}

fn default_star() -> String {
    "*".to_string()
}

fn default_rule_pack_version() -> u32 {
    1
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuleEventKind {
    #[default]
    Transaction,
    BillPaid,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum NumberFormat {
    #[default]
    Auto,
    DotDecimal,
    CommaDecimal,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TargetField {
    Text,
    Title,
    #[default]
    TitleText,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TransactionRuleDef {
    pub id: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "default_star")]
    pub package: String,
    #[serde(default = "default_star")]
    pub rule_template: String,
    #[serde(default)]
    pub source_types: Vec<MoneySourceType>,
    #[serde(default)]
    pub event_kind: RuleEventKind,
    #[serde(default)]
    pub direction: Option<TxDirection>,
    #[serde(default)]
    pub number_format: NumberFormat,
    #[serde(default)]
    pub target_field: TargetField,
    pub pattern: String,
    #[serde(default)]
    pub exclude_pattern: Option<String>,
    #[serde(default)]
    pub default_category: Option<String>,
    #[serde(default)]
    pub transfer_hint: Option<TransferHint>,
    #[serde(default)]
    pub default_currency: Option<String>,
    #[serde(default)]
    pub is_statement_due: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TransactionRulePack {
    #[serde(default = "default_rule_pack_version")]
    pub version: u32,
    #[serde(default)]
    pub rules: Vec<TransactionRuleDef>,
}

#[derive(Clone, Debug)]
pub struct CompiledRule {
    pub def: TransactionRuleDef,
    pub regex: Regex,
    pub exclude_regex: Option<Regex>,
    pub from_user: bool,
}

#[derive(Clone, Debug, Default)]
pub struct LoadedRules {
    pub rules: Vec<CompiledRule>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParsedMoneyOutcome {
    Transaction(StoredTransaction),
    LiabilityEvent(LiabilityEvent),
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MoneyVaultSnapshot {
    pub transactions: Vec<StoredTransaction>,
    pub liability_events: Vec<LiabilityEvent>,
    pub sub_account_nicknames: HashMap<String, String>,
    pub currencies: Vec<String>,
    pub rule_warnings: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LiabilityResolveResult {
    pub event: LiabilityEvent,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CorrectionResult {
    pub updated_count: usize,
    pub transaction: StoredTransaction,
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn sha256_hex_parts(parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hex_encode(&hasher.finalize())
}

fn compile_rule_pack(
    raw_json: &str,
    from_user: bool,
) -> Result<(Vec<CompiledRule>, Vec<String>), String> {
    let pack: TransactionRulePack =
        serde_json::from_str(raw_json).map_err(|e| format!("Invalid rule pack JSON: {e}"))?;
    let mut compiled = Vec::new();
    let mut warnings = Vec::new();

    for def in pack.rules {
        let id_clean = def.id.trim();
        if id_clean.is_empty() || def.source_types.is_empty() {
            let safe_id = if id_clean.is_empty() {
                "unnamed-rule".to_string()
            } else {
                id_clean.to_string()
            };
            warnings.push(safe_id);
            continue;
        }
        if !def.enabled {
            continue;
        }
        let regex = match Regex::new(&def.pattern) {
            Ok(r) => r,
            Err(_) => {
                #[cfg(debug_assertions)]
                eprintln!("[phone_hub::money] skipped invalid rule id={}", def.id);
                warnings.push(def.id.clone());
                continue;
            }
        };
        let exclude_regex = match def.exclude_pattern.as_deref() {
            Some(pat) if !pat.trim().is_empty() => match Regex::new(pat) {
                Ok(r) => Some(r),
                Err(_) => {
                    #[cfg(debug_assertions)]
                    eprintln!("[phone_hub::money] skipped invalid rule id={}", def.id);
                    warnings.push(def.id.clone());
                    continue;
                }
            },
            _ => None,
        };
        compiled.push(CompiledRule {
            def,
            regex,
            exclude_regex,
            from_user,
        });
    }

    Ok((compiled, warnings))
}

pub fn load_rules_from_strings(default_json: &str, user_json: Option<&str>) -> LoadedRules {
    let mut all_rules = Vec::new();
    let mut warnings = Vec::new();

    if let Some(ujson) = user_json {
        match compile_rule_pack(ujson, true) {
            Ok((user_rules, user_warnings)) => {
                all_rules.extend(user_rules);
                warnings.extend(user_warnings);
            }
            Err(_) => {
                #[cfg(debug_assertions)]
                eprintln!("[phone_hub::money] skipped unreadable user rule pack");
                warnings.push("user-rule-pack".to_string());
            }
        }
    }

    if let Ok((def_rules, def_warnings)) = compile_rule_pack(default_json, false) {
        all_rules.extend(def_rules);
        warnings.extend(def_warnings);
    }

    LoadedRules {
        rules: all_rules,
        warnings,
    }
}

pub fn load_rules_for_dir(base_dir: Option<&Path>) -> LoadedRules {
    let user_json = base_dir
        .map(|dir| dir.join(USER_RULES_FILENAME))
        .filter(|path| path.exists())
        .and_then(|path| fs::read_to_string(path).ok());
    load_rules_from_strings(DEFAULT_RULES_JSON, user_json.as_deref())
}

pub fn normalize_currency(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.eq_ignore_ascii_case("rp") || trimmed.eq_ignore_ascii_case("rp.") {
        return "IDR".to_string();
    }
    if trimmed == "$" {
        return "USD".to_string();
    }
    if trimmed.eq_ignore_ascii_case("rm") {
        return "MYR".to_string();
    }
    trimmed.to_ascii_uppercase()
}

fn is_indonesian_currency(currency_hint: Option<&str>) -> bool {
    let Some(c) = currency_hint else {
        return false;
    };
    let norm = normalize_currency(c);
    norm == "IDR"
}

fn parse_digits_u64(s: &str) -> Option<i64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<i64>().ok()
}

fn parse_with_decimal_sep(raw: &str, decimal_sep: char, thousands_sep: char) -> Option<(i64, u8)> {
    let mut dec_parts = raw.split(decimal_sep);
    let int_part_raw = dec_parts.next()?;
    let frac_part_raw = dec_parts.next();
    if dec_parts.next().is_some() {
        return None;
    }
    if int_part_raw.is_empty()
        || int_part_raw.starts_with(thousands_sep)
        || int_part_raw.ends_with(thousands_sep)
    {
        return None;
    }
    if int_part_raw.contains(&format!("{thousands_sep}{thousands_sep}")) {
        return None;
    }
    let int_clean: String = int_part_raw
        .chars()
        .filter(|&c| c != thousands_sep)
        .collect();
    let int_val = parse_digits_u64(&int_clean)?;

    let frac_val = match frac_part_raw {
        None => 0_i64,
        Some(f) => {
            if f.is_empty() || f.len() > 2 || !f.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let d = f.parse::<i64>().ok()?;
            if f.len() == 1 {
                d * 10
            } else {
                d
            }
        }
    };

    let minor = int_val.checked_mul(100)?.checked_add(frac_val)?;
    Some((minor, DEFAULT_CURRENCY_SCALE))
}

fn parse_thousands_only(raw: &str, sep: char) -> Option<(i64, u8)> {
    let groups: Vec<&str> = raw.split(sep).collect();
    if groups.len() < 2 {
        return None;
    }
    if groups[0].is_empty() || groups[0].len() > 3 || !groups[0].bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    for g in &groups[1..] {
        if g.len() != 3 || !g.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
    }
    let clean: String = groups.concat();
    let int_val = parse_digits_u64(&clean)?;
    Some((int_val.checked_mul(100)?, DEFAULT_CURRENCY_SCALE))
}

/// Parses localized amounts into exact integer minor units (`i64`) + `scale` (2).
pub fn parse_localized_amount(
    raw: &str,
    currency_hint: Option<&str>,
    format: NumberFormat,
) -> Option<(i64, u8)> {
    let s = raw.trim().trim_end_matches(['.', ',']);
    if s.is_empty() {
        return None;
    }
    if !s
        .bytes()
        .all(|b| b.is_ascii_digit() || b == b'.' || b == b',')
    {
        return None;
    }

    let dot_count = s.matches('.').count();
    let comma_count = s.matches(',').count();

    if dot_count == 0 && comma_count == 0 {
        let val = parse_digits_u64(s)?;
        return Some((val.checked_mul(100)?, DEFAULT_CURRENCY_SCALE));
    }

    match format {
        NumberFormat::DotDecimal => {
            if dot_count == 0 && comma_count >= 1 {
                return parse_thousands_only(s, ',');
            }
            parse_with_decimal_sep(s, '.', ',')
        }
        NumberFormat::CommaDecimal => {
            if comma_count == 0 && dot_count >= 1 {
                return parse_thousands_only(s, '.');
            }
            parse_with_decimal_sep(s, ',', '.')
        }
        NumberFormat::Auto => {
            if dot_count > 0 && comma_count > 0 {
                let last_dot = s.rfind('.');
                let last_comma = s.rfind(',');
                if last_dot > last_comma {
                    return parse_with_decimal_sep(s, '.', ',');
                }
                return parse_with_decimal_sep(s, ',', '.');
            }

            if dot_count > 1 {
                return parse_thousands_only(s, '.');
            }
            if comma_count > 1 {
                return parse_thousands_only(s, ',');
            }

            if dot_count == 1 {
                let (_, after) = s.split_once('.')?;
                if after.len() == 3 && is_indonesian_currency(currency_hint) {
                    return parse_thousands_only(s, '.');
                }
                if after.len() == 3 && currency_hint.is_none() {
                    if let Some(res) = parse_thousands_only(s, '.') {
                        return Some(res);
                    }
                }
                return parse_with_decimal_sep(s, '.', ',');
            }

            if comma_count == 1 {
                let (_, after) = s.split_once(',')?;
                if after.len() == 3 {
                    if let Some(res) = parse_thousands_only(s, ',') {
                        return Some(res);
                    }
                }
                return parse_with_decimal_sep(s, ',', '.');
            }

            None
        }
    }
}

pub fn compute_sub_account_key(
    rule_salt: &[u8; 32],
    package: &str,
    source_id: &str,
    card_last4_raw: &str,
) -> Option<String> {
    let digits: String = card_last4_raw
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect();
    if digits.len() < 4 {
        return None;
    }
    let last4 = &digits[digits.len() - 4..];
    let hash = sha256_hex_parts(&[
        rule_salt,
        b"sub_account::",
        package.as_bytes(),
        b"::",
        source_id.as_bytes(),
        b"::",
        last4.as_bytes(),
    ]);
    Some(format!("sa_{hash}"))
}

pub fn normalize_counterparty_pattern(raw: &str) -> String {
    let trimmed = raw
        .trim()
        .trim_matches(|c: char| matches!(c, '.' | ',' | ';' | ':' | '-' | '—' | '–'));
    let collapsed = trimmed
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    collapsed
}

pub fn compute_similarity_hash(
    rule_salt: &[u8; 32],
    package: &str,
    direction: TxDirection,
    counterparty_raw: &str,
) -> Option<String> {
    let normalized = normalize_counterparty_pattern(counterparty_raw);
    if normalized.is_empty() {
        return None;
    }
    let dir_str = match direction {
        TxDirection::In => "in",
        TxDirection::Out => "out",
    };
    Some(sha256_hex_parts(&[
        rule_salt,
        package.as_bytes(),
        b"::",
        dir_str.as_bytes(),
        b"::",
        normalized.as_bytes(),
    ]))
}

pub fn make_stable_record_id(
    prefix: &str,
    hw_serial: &str,
    key: &str,
    posted_unix: i64,
    rule_id: &str,
) -> String {
    let hash = sha256_hex_parts(&[
        hw_serial.as_bytes(),
        b"::",
        key.as_bytes(),
        b"::",
        posted_unix.to_string().as_bytes(),
        b"::",
        rule_id.as_bytes(),
    ]);
    format!("{prefix}_{}", &hash[..24])
}

fn is_generic_template(template: &str) -> bool {
    matches!(
        template,
        "generic_bank" | "generic_ewallet" | "generic_paylater" | "generic_credit_card"
    )
}

fn rule_target_text(item: &PhoneHubNotification, field: TargetField) -> String {
    match field {
        TargetField::Text => item.text.clone(),
        TargetField::Title => item.title.clone(),
        TargetField::TitleText => {
            if item.title.trim().is_empty() {
                item.text.clone()
            } else if item.text.trim().is_empty() {
                item.title.clone()
            } else {
                format!("{}\n{}", item.title, item.text)
            }
        }
    }
}

fn clean_category(raw: &str) -> Option<String> {
    let cleaned = raw
        .trim()
        .trim_end_matches(['.', ',', ';', ':', '!'])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

pub fn parse_notification_for_sources(
    loaded: &LoadedRules,
    hw_serial: &str,
    item: &PhoneHubNotification,
    policy: &AppNotificationPolicy,
    now_unix: i64,
    rule_salt: &[u8; 32],
) -> Option<ParsedMoneyOutcome> {
    if policy.retention == RetentionPolicy::None {
        return None;
    }

    let enabled_sources: Vec<(usize, &MoneySourceConfig)> = policy
        .money_sources
        .iter()
        .enumerate()
        .filter(|(_, s)| s.enabled)
        .collect();
    if enabled_sources.is_empty() {
        return None;
    }

    // Gather all (specificity, priority, source_idx, rule) candidates across enabled sources.
    #[derive(Clone, Copy)]
    struct Candidate<'a> {
        pkg_spec: u8,
        tmpl_spec: u8,
        user_tier: u8,
        priority: i32,
        source_idx: usize,
        source: &'a MoneySourceConfig,
        rule: &'a CompiledRule,
    }

    let mut candidates: Vec<Candidate<'_>> = Vec::new();
    for (source_idx, source) in &enabled_sources {
        for rule in &loaded.rules {
            if !rule.def.source_types.contains(&source.source_type) {
                continue;
            }
            let pkg_match = rule.def.package == item.package || rule.def.package == "*";
            if !pkg_match {
                continue;
            }
            let pkg_spec = u8::from(rule.def.package == item.package);

            let tmpl_spec = if rule.def.rule_template == source.rule_template {
                2
            } else if rule.def.rule_template == "*"
                && (rule.def.package == item.package || is_generic_template(&source.rule_template))
            {
                0
            } else {
                continue;
            };

            candidates.push(Candidate {
                pkg_spec,
                tmpl_spec,
                user_tier: u8::from(rule.from_user),
                priority: rule.def.priority,
                source_idx: *source_idx,
                source,
                rule,
            });
        }
    }

    candidates.sort_by(|a, b| {
        b.pkg_spec
            .cmp(&a.pkg_spec)
            .then_with(|| b.tmpl_spec.cmp(&a.tmpl_spec))
            .then_with(|| b.user_tier.cmp(&a.user_tier))
            .then_with(|| b.priority.cmp(&a.priority))
            .then_with(|| a.source_idx.cmp(&b.source_idx))
            .then_with(|| a.rule.def.id.cmp(&b.rule.def.id))
    });

    let timestamp_secs = if item.posted_unix > 0 {
        item.posted_unix
    } else {
        now_unix
    };
    let timestamp_ms = timestamp_secs.saturating_mul(1000);
    let dedupe_id = make_dedupe_id(hw_serial, &item.key, item.posted_unix);

    for cand in candidates {
        let target = rule_target_text(item, cand.rule.def.target_field);
        if let Some(ref excl) = cand.rule.exclude_regex {
            if excl.is_match(&target) {
                continue;
            }
        }
        let Some(caps) = cand.rule.regex.captures(&target) else {
            continue;
        };

        let sub_account = caps.name("cardLast4").and_then(|m| {
            compute_sub_account_key(rule_salt, &item.package, &cand.source.id, m.as_str())
        });

        match cand.rule.def.event_kind {
            RuleEventKind::BillPaid => {
                if !matches!(
                    cand.source.source_type,
                    MoneySourceType::Paylater | MoneySourceType::CreditCard
                ) {
                    continue;
                }
                let id = make_stable_record_id(
                    "le",
                    hw_serial,
                    &item.key,
                    item.posted_unix,
                    &cand.rule.def.id,
                );
                return Some(ParsedMoneyOutcome::LiabilityEvent(LiabilityEvent {
                    id,
                    dedupe_id,
                    hw_serial: hw_serial.to_string(),
                    package: item.package.clone(),
                    source_id: cand.source.id.clone(),
                    app_label: item.app_label.clone(),
                    source_type: cand.source.source_type,
                    event_type: LiabilityEventType::BillPaid,
                    sub_account,
                    status: LiabilityEventStatus::UnmatchedNeedsReview,
                    linked_tx_id: None,
                    resolved_amount_minor: None,
                    resolved_currency: None,
                    scale: None,
                    manual_locked: false,
                    duplicate_of: None,
                    timestamp_ms,
                    rule_id: cand.rule.def.id.clone(),
                }));
            }
            RuleEventKind::Transaction => {
                let Some(direction) = cand.rule.def.direction else {
                    continue;
                };

                let raw_currency = caps.name("currency").map(|m| m.as_str()).or(cand
                    .rule
                    .def
                    .default_currency
                    .as_deref());
                let Some(cur_str) = raw_currency else {
                    continue;
                };
                let currency = normalize_currency(cur_str);
                if currency.is_empty() {
                    continue;
                }

                if cand.rule.def.is_statement_due {
                    let Some(due_raw) = caps.name("billDue").map(|m| m.as_str()) else {
                        continue;
                    };
                    let Some((due_minor, scale)) = parse_localized_amount(
                        due_raw,
                        Some(&currency),
                        cand.rule.def.number_format,
                    ) else {
                        continue;
                    };
                    if due_minor <= 0 {
                        continue;
                    }
                    let id = make_stable_record_id(
                        "tx",
                        hw_serial,
                        &item.key,
                        item.posted_unix,
                        &cand.rule.def.id,
                    );
                    return Some(ParsedMoneyOutcome::Transaction(StoredTransaction {
                        id,
                        dedupe_id,
                        hw_serial: hw_serial.to_string(),
                        package: item.package.clone(),
                        source_id: cand.source.id.clone(),
                        app_label: item.app_label.clone(),
                        source_type: cand.source.source_type,
                        sub_account,
                        direction,
                        amount_minor: 0,
                        scale,
                        currency,
                        category: cand
                            .rule
                            .def
                            .default_category
                            .clone()
                            .unwrap_or_else(|| "Statement due".to_string()),
                        classification: TxClassification::Ignored,
                        manual_classification: None,
                        manual_category: None,
                        transfer_pair_id: None,
                        linked_liability_event_id: None,
                        linked_liability_source_id: None,
                        linked_sub_account: None,
                        transfer_hint: None,
                        similarity_hash: None,
                        statement_due_minor: Some(due_minor),
                        statement_due_unix: Some(timestamp_secs),
                        timestamp_ms,
                        rule_id: cand.rule.def.id.clone(),
                    }));
                }

                let Some(amt_raw) = caps.name("amount").map(|m| m.as_str()) else {
                    continue;
                };
                let Some((amount_minor, scale)) =
                    parse_localized_amount(amt_raw, Some(&currency), cand.rule.def.number_format)
                else {
                    continue;
                };
                if amount_minor <= 0 {
                    continue;
                }

                let bill_due_minor = caps
                    .name("billDue")
                    .and_then(|m| {
                        parse_localized_amount(
                            m.as_str(),
                            Some(&currency),
                            cand.rule.def.number_format,
                        )
                    })
                    .map(|(v, _)| v);

                let category = caps
                    .name("category")
                    .and_then(|m| clean_category(m.as_str()))
                    .or_else(|| cand.rule.def.default_category.clone())
                    .unwrap_or_else(|| "Uncategorized".to_string());

                let similarity_hash = caps.name("counterparty").and_then(|m| {
                    compute_similarity_hash(rule_salt, &item.package, direction, m.as_str())
                });

                let initial_classification = if direction == TxDirection::Out
                    && cand.rule.def.transfer_hint == Some(TransferHint::Repayment)
                {
                    TxClassification::DebtPayment
                } else {
                    TxClassification::Normal
                };

                let id = make_stable_record_id(
                    "tx",
                    hw_serial,
                    &item.key,
                    item.posted_unix,
                    &cand.rule.def.id,
                );
                return Some(ParsedMoneyOutcome::Transaction(StoredTransaction {
                    id,
                    dedupe_id,
                    hw_serial: hw_serial.to_string(),
                    package: item.package.clone(),
                    source_id: cand.source.id.clone(),
                    app_label: item.app_label.clone(),
                    source_type: cand.source.source_type,
                    sub_account,
                    direction,
                    amount_minor,
                    scale,
                    currency,
                    category,
                    classification: initial_classification,
                    manual_classification: None,
                    manual_category: None,
                    transfer_pair_id: None,
                    linked_liability_event_id: None,
                    linked_liability_source_id: None,
                    linked_sub_account: None,
                    transfer_hint: cand.rule.def.transfer_hint,
                    similarity_hash,
                    statement_due_minor: bill_due_minor,
                    statement_due_unix: bill_due_minor.map(|_| timestamp_secs),
                    timestamp_ms,
                    rule_id: cand.rule.def.id.clone(),
                }));
            }
        }
    }

    None
}

pub fn apply_matching_correction_rule(tx: &mut StoredTransaction, rules: &[CorrectionRule]) {
    let Some(ref sim_hash) = tx.similarity_hash else {
        return;
    };
    if let Some(rule) = rules.iter().find(|r| {
        r.package == tx.package && r.direction == tx.direction && &r.similarity_hash == sim_hash
    }) {
        if let Some(cls) = rule.classification {
            tx.manual_classification = Some(cls);
            tx.classification = cls;
        }
        if let Some(ref cat) = rule.category {
            tx.manual_category = Some(cat.clone());
            tx.category = cat.clone();
        }
    }
}

fn deterministic_pair_id(out_id: &str, in_id: &str) -> String {
    let hash = sha256_hex_parts(&[out_id.as_bytes(), b"::", in_id.as_bytes()]);
    format!("pair_{}", &hash[..20])
}

fn is_asset_source(st: MoneySourceType) -> bool {
    matches!(st, MoneySourceType::Bank | MoneySourceType::Ewallet)
}

fn is_liability_source(st: MoneySourceType) -> bool {
    matches!(st, MoneySourceType::Paylater | MoneySourceType::CreditCard)
}

fn is_statement_due_record(tx: &StoredTransaction) -> bool {
    tx.amount_minor == 0 && tx.statement_due_minor.is_some()
}

fn pass2_hint_tier(tx: &StoredTransaction) -> u8 {
    if matches!(
        tx.transfer_hint,
        Some(
            TransferHint::Payment
                | TransferHint::VirtualAccount
                | TransferHint::Bill
                | TransferHint::Repayment
        )
    ) || matches!(tx.category.as_str(), "Payment" | "Bill" | "Debt payment")
    {
        0
    } else {
        1
    }
}

/// Runs deterministic bill-paid collapse + Pass 1/2/3 transfer/debt matching.
/// Returns `true` if any transaction or liability event changed.
pub fn recompute_matches(vault: &mut TransactionsVault, window_minutes: u32) -> bool {
    let before_txs = vault.transactions.clone();
    let before_evs = vault.liability_events.clone();

    let clamped_mins = window_minutes.clamp(1, 60) as i64;
    let window_ms = clamped_mins * 60 * 1000;

    // 1. Collect manually locked transaction IDs (manual classification, statement-due, or manually linked liability event).
    let mut locked_tx_ids: HashSet<String> = HashSet::new();
    let mut manual_linked_tx_meta: HashMap<String, (String, String, Option<String>)> =
        HashMap::new();
    let mut locked_event_ids: HashSet<String> = HashSet::new();

    for ev in &vault.liability_events {
        if ev.manual_locked {
            locked_event_ids.insert(ev.id.clone());
            if ev.status == LiabilityEventStatus::Linked {
                if let Some(ref tx_id) = ev.linked_tx_id {
                    locked_tx_ids.insert(tx_id.clone());
                    manual_linked_tx_meta.insert(
                        tx_id.clone(),
                        (ev.id.clone(), ev.source_id.clone(), ev.sub_account.clone()),
                    );
                }
            }
        }
    }

    for tx in &mut vault.transactions {
        if is_statement_due_record(tx) {
            tx.classification = TxClassification::Ignored;
            locked_tx_ids.insert(tx.id.clone());
            continue;
        }
        if let Some(mc) = tx.manual_classification {
            tx.classification = mc;
            if let Some((ev_id, src_id, sa)) = manual_linked_tx_meta.get(&tx.id) {
                tx.linked_liability_event_id = Some(ev_id.clone());
                tx.linked_liability_source_id = Some(src_id.clone());
                tx.linked_sub_account = sa.clone();
                tx.transfer_pair_id = None;
            } else if mc != TxClassification::Transfer && mc != TxClassification::DebtPayment {
                tx.transfer_pair_id = None;
                tx.linked_liability_event_id = None;
                tx.linked_liability_source_id = None;
                tx.linked_sub_account = None;
            }
            locked_tx_ids.insert(tx.id.clone());
            continue;
        }
        if let Some((ev_id, src_id, sa)) = manual_linked_tx_meta.get(&tx.id) {
            tx.classification = TxClassification::DebtPayment;
            tx.transfer_pair_id = None;
            tx.linked_liability_event_id = Some(ev_id.clone());
            tx.linked_liability_source_id = Some(src_id.clone());
            tx.linked_sub_account = sa.clone();
            locked_tx_ids.insert(tx.id.clone());
            continue;
        }

        // Reset auto-matched fields
        tx.classification = TxClassification::Normal;
        tx.transfer_pair_id = None;
        tx.linked_liability_event_id = None;
        tx.linked_liability_source_id = None;
        tx.linked_sub_account = None;
    }

    // Reset unlocked liability events
    for ev in &mut vault.liability_events {
        if ev.manual_locked {
            continue;
        }
        ev.duplicate_of = None;
        ev.status = LiabilityEventStatus::UnmatchedNeedsReview;
        ev.linked_tx_id = None;
        ev.resolved_amount_minor = None;
        ev.resolved_currency = None;
        ev.scale = None;
    }

    // 2. Bill-paid collapse: same (package, source_id, sub_account) within +-W collapses to earliest.
    let mut ev_indices: Vec<usize> = (0..vault.liability_events.len()).collect();
    ev_indices.sort_by(|&a, &b| {
        let ea = &vault.liability_events[a];
        let eb = &vault.liability_events[b];
        ea.timestamp_ms
            .cmp(&eb.timestamp_ms)
            .then_with(|| ea.id.cmp(&eb.id))
    });

    let mut keepers: Vec<(String, String, Option<String>, i64, String)> = Vec::new();
    for idx in ev_indices {
        let ev = &mut vault.liability_events[idx];
        if ev.event_type != LiabilityEventType::BillPaid {
            continue;
        }
        if ev.manual_locked && ev.status == LiabilityEventStatus::Ignored {
            continue;
        }
        if !ev.manual_locked {
            if let Some((_, _, _, _, keeper_id)) = keepers.iter().find(|(pkg, src, sa, ts, _)| {
                pkg == &ev.package
                    && src == &ev.source_id
                    && sa == &ev.sub_account
                    && (ev.timestamp_ms - *ts).abs() <= window_ms
            }) {
                ev.duplicate_of = Some(keeper_id.clone());
                ev.status = LiabilityEventStatus::Ignored;
                locked_event_ids.insert(ev.id.clone());
                continue;
            }
        }
        keepers.push((
            ev.package.clone(),
            ev.source_id.clone(),
            ev.sub_account.clone(),
            ev.timestamp_ms,
            ev.id.clone(),
        ));
    }

    let mut matched_tx_ids: HashSet<String> = locked_tx_ids;
    let mut matched_event_ids: HashSet<String> = locked_event_ids;

    // 3. Pass 1: Exact-amount matching (asset OUT <-> asset IN or amounted liability IN)
    #[derive(Clone)]
    struct Pass1Cand {
        out_idx: usize,
        in_idx: usize,
        dt: i64,
        liability_tie: u8,
        lag_penalty: u8,
        out_ts: i64,
        out_id: String,
        in_id: String,
    }

    let mut pass1_cands: Vec<Pass1Cand> = Vec::new();
    for (out_idx, out_tx) in vault.transactions.iter().enumerate() {
        if matched_tx_ids.contains(&out_tx.id)
            || out_tx.direction != TxDirection::Out
            || !is_asset_source(out_tx.source_type)
            || out_tx.amount_minor <= 0
        {
            continue;
        }
        for (in_idx, in_tx) in vault.transactions.iter().enumerate() {
            if out_idx == in_idx
                || matched_tx_ids.contains(&in_tx.id)
                || in_tx.direction != TxDirection::In
                || (!is_asset_source(in_tx.source_type) && !is_liability_source(in_tx.source_type))
                || in_tx.amount_minor <= 0
            {
                continue;
            }
            if (out_tx.package.as_str(), out_tx.source_id.as_str())
                == (in_tx.package.as_str(), in_tx.source_id.as_str())
            {
                continue;
            }
            if out_tx.currency != in_tx.currency
                || out_tx.amount_minor != in_tx.amount_minor
                || out_tx.scale != in_tx.scale
            {
                continue;
            }
            let dt = (out_tx.timestamp_ms - in_tx.timestamp_ms).abs();
            if dt > window_ms {
                continue;
            }
            pass1_cands.push(Pass1Cand {
                out_idx,
                in_idx,
                dt,
                liability_tie: if is_liability_source(in_tx.source_type) {
                    0
                } else {
                    1
                },
                lag_penalty: if in_tx.timestamp_ms >= out_tx.timestamp_ms {
                    0
                } else {
                    1
                },
                out_ts: out_tx.timestamp_ms,
                out_id: out_tx.id.clone(),
                in_id: in_tx.id.clone(),
            });
        }
    }

    pass1_cands.sort_by(|a, b| {
        a.dt.cmp(&b.dt)
            .then_with(|| a.liability_tie.cmp(&b.liability_tie))
            .then_with(|| a.lag_penalty.cmp(&b.lag_penalty))
            .then_with(|| a.out_ts.cmp(&b.out_ts))
            .then_with(|| a.out_id.cmp(&b.out_id))
            .then_with(|| a.in_id.cmp(&b.in_id))
    });

    for cand in pass1_cands {
        if matched_tx_ids.contains(&cand.out_id) || matched_tx_ids.contains(&cand.in_id) {
            continue;
        }
        matched_tx_ids.insert(cand.out_id.clone());
        matched_tx_ids.insert(cand.in_id.clone());
        let pair_id = deterministic_pair_id(&cand.out_id, &cand.in_id);

        let in_source_type = vault.transactions[cand.in_idx].source_type;
        let in_source_id = vault.transactions[cand.in_idx].source_id.clone();
        let in_sub_account = vault.transactions[cand.in_idx].sub_account.clone();

        if is_liability_source(in_source_type) {
            let out_tx = &mut vault.transactions[cand.out_idx];
            out_tx.classification = TxClassification::DebtPayment;
            out_tx.transfer_pair_id = Some(pair_id.clone());
            out_tx.linked_liability_source_id = Some(in_source_id);
            out_tx.linked_sub_account = in_sub_account;

            let in_tx = &mut vault.transactions[cand.in_idx];
            in_tx.classification = TxClassification::RepaymentReceived;
            in_tx.transfer_pair_id = Some(pair_id);
        } else {
            vault.transactions[cand.out_idx].classification = TxClassification::Transfer;
            vault.transactions[cand.out_idx].transfer_pair_id = Some(pair_id.clone());
            vault.transactions[cand.in_idx].classification = TxClassification::Transfer;
            vault.transactions[cand.in_idx].transfer_pair_id = Some(pair_id);
        }
    }

    // 4. Pass 2: Amount-less bill_paid matching
    #[derive(Clone)]
    struct Pass2Cand {
        out_idx: usize,
        ev_idx: usize,
        hint_tier: u8,
        dt: i64,
        lag_penalty: u8,
        out_ts: i64,
        out_id: String,
        ev_id: String,
    }

    let mut pass2_cands: Vec<Pass2Cand> = Vec::new();
    for (out_idx, out_tx) in vault.transactions.iter().enumerate() {
        if matched_tx_ids.contains(&out_tx.id)
            || out_tx.direction != TxDirection::Out
            || !is_asset_source(out_tx.source_type)
            || out_tx.amount_minor <= 0
        {
            continue;
        }
        for (ev_idx, ev) in vault.liability_events.iter().enumerate() {
            if matched_event_ids.contains(&ev.id)
                || ev.event_type != LiabilityEventType::BillPaid
                || ev.duplicate_of.is_some()
            {
                continue;
            }
            if (out_tx.package.as_str(), out_tx.source_id.as_str())
                == (ev.package.as_str(), ev.source_id.as_str())
            {
                continue;
            }
            let dt = (out_tx.timestamp_ms - ev.timestamp_ms).abs();
            if dt > window_ms {
                continue;
            }
            pass2_cands.push(Pass2Cand {
                out_idx,
                ev_idx,
                hint_tier: pass2_hint_tier(out_tx),
                dt,
                lag_penalty: if ev.timestamp_ms >= out_tx.timestamp_ms {
                    0
                } else {
                    1
                },
                out_ts: out_tx.timestamp_ms,
                out_id: out_tx.id.clone(),
                ev_id: ev.id.clone(),
            });
        }
    }

    pass2_cands.sort_by(|a, b| {
        a.hint_tier
            .cmp(&b.hint_tier)
            .then_with(|| a.dt.cmp(&b.dt))
            .then_with(|| a.lag_penalty.cmp(&b.lag_penalty))
            .then_with(|| a.out_ts.cmp(&b.out_ts))
            .then_with(|| a.out_id.cmp(&b.out_id))
            .then_with(|| a.ev_id.cmp(&b.ev_id))
    });

    for cand in pass2_cands {
        if matched_tx_ids.contains(&cand.out_id) || matched_event_ids.contains(&cand.ev_id) {
            continue;
        }
        matched_tx_ids.insert(cand.out_id.clone());
        matched_event_ids.insert(cand.ev_id.clone());

        let ev_id = vault.liability_events[cand.ev_idx].id.clone();
        let ev_source_id = vault.liability_events[cand.ev_idx].source_id.clone();
        let ev_sub_account = vault.liability_events[cand.ev_idx].sub_account.clone();

        let out_amount = vault.transactions[cand.out_idx].amount_minor;
        let out_currency = vault.transactions[cand.out_idx].currency.clone();
        let out_scale = vault.transactions[cand.out_idx].scale;

        let out_tx = &mut vault.transactions[cand.out_idx];
        out_tx.classification = TxClassification::DebtPayment;
        out_tx.linked_liability_event_id = Some(ev_id);
        out_tx.linked_liability_source_id = Some(ev_source_id);
        out_tx.linked_sub_account = ev_sub_account;

        let ev = &mut vault.liability_events[cand.ev_idx];
        ev.status = LiabilityEventStatus::Linked;
        ev.linked_tx_id = Some(cand.out_id);
        ev.resolved_amount_minor = Some(out_amount);
        ev.resolved_currency = Some(out_currency);
        ev.scale = Some(out_scale);
    }

    // 5. Pass 3: Fallback review flags & unpaired repayment OUT preservation
    for tx in &mut vault.transactions {
        if matched_tx_ids.contains(&tx.id) {
            continue;
        }
        if tx.direction == TxDirection::Out {
            if tx.transfer_hint == Some(TransferHint::Repayment) {
                tx.classification = TxClassification::DebtPayment;
            } else if tx.source_type == MoneySourceType::Bank
                && matches!(
                    tx.transfer_hint,
                    Some(TransferHint::VirtualAccount | TransferHint::TopUp)
                )
            {
                tx.classification = TxClassification::PossibleTransfer;
            } else {
                tx.classification = TxClassification::Normal;
            }
        } else {
            tx.classification = TxClassification::Normal;
        }
    }

    for ev in &mut vault.liability_events {
        if matched_event_ids.contains(&ev.id) {
            continue;
        }
        ev.status = LiabilityEventStatus::UnmatchedNeedsReview;
    }

    vault.transactions != before_txs || vault.liability_events != before_evs
}

pub fn build_money_snapshot(
    store: &mut VaultStore,
    hw_serial: Option<&str>,
    currency: Option<&str>,
    from_ms: Option<i64>,
    to_ms: Option<i64>,
) -> Result<MoneyVaultSnapshot, String> {
    if !store.config().history_enabled {
        return Ok(MoneyVaultSnapshot::default());
    }
    store.ensure_loaded()?;
    let loaded_rules = load_rules_for_dir(store.base_dir().map(|p| p.as_path()));
    let vault = store.transactions_vault();

    let matches_hw_and_range = |hw: &str, ts_ms: i64| -> bool {
        if let Some(target_hw) = hw_serial {
            if hw != target_hw {
                return false;
            }
        }
        if let Some(from) = from_ms {
            if ts_ms < from {
                return false;
            }
        }
        if let Some(to) = to_ms {
            if ts_ms > to {
                return false;
            }
        }
        true
    };

    let mut currency_set: HashSet<String> = HashSet::new();
    for tx in &vault.transactions {
        if is_statement_due_record(tx) || tx.classification == TxClassification::Ignored {
            continue;
        }
        if matches_hw_and_range(&tx.hw_serial, tx.timestamp_ms) && !tx.currency.is_empty() {
            currency_set.insert(tx.currency.clone());
        }
    }
    let mut currencies: Vec<String> = currency_set.into_iter().collect();
    currencies.sort();

    let mut transactions: Vec<StoredTransaction> = vault
        .transactions
        .iter()
        .filter(|tx| {
            !is_statement_due_record(tx)
                && matches_hw_and_range(&tx.hw_serial, tx.timestamp_ms)
                && currency.is_none_or(|c| tx.currency == c)
        })
        .cloned()
        .collect();
    transactions.sort_by(|a, b| {
        b.timestamp_ms
            .cmp(&a.timestamp_ms)
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut liability_events: Vec<LiabilityEvent> = vault
        .liability_events
        .iter()
        .filter(|ev| {
            ev.duplicate_of.is_none()
                && matches_hw_and_range(&ev.hw_serial, ev.timestamp_ms)
                && currency.is_none_or(|c| {
                    ev.resolved_currency
                        .as_deref()
                        .is_none_or(|ev_cur| ev_cur == c)
                })
        })
        .cloned()
        .collect();
    liability_events.sort_by(|a, b| {
        b.timestamp_ms
            .cmp(&a.timestamp_ms)
            .then_with(|| a.id.cmp(&b.id))
    });

    Ok(MoneyVaultSnapshot {
        transactions,
        liability_events,
        sub_account_nicknames: vault.sub_account_nicknames.clone(),
        currencies,
        rule_warnings: loaded_rules.warnings,
    })
}

pub fn resolve_liability_event_in_vault(
    store: &mut VaultStore,
    event_id: &str,
    linked_tx_id: Option<&str>,
    ignore: bool,
) -> Result<LiabilityResolveResult, String> {
    if !store.config().history_enabled {
        return Err("Notification storage is disabled".to_string());
    }
    store.ensure_loaded()?;
    let window = store.config().transfer_match_window_minutes;

    let (linked_amount, linked_currency, linked_scale) = if !ignore {
        if let Some(tx_id) = linked_tx_id {
            let tx = store
                .transactions_vault()
                .transactions
                .iter()
                .find(|t| t.id == tx_id)
                .ok_or_else(|| "Target transaction not found".to_string())?;
            if tx.direction != TxDirection::Out || !is_asset_source(tx.source_type) {
                return Err("Linked transaction must be a bank or e-wallet outflow".to_string());
            }
            (
                Some(tx.amount_minor),
                Some(tx.currency.clone()),
                Some(tx.scale),
            )
        } else {
            (None, None, None)
        }
    } else {
        (None, None, None)
    };

    let vault = store.transactions_vault_mut();
    let Some(ev_idx) = vault.liability_events.iter().position(|e| e.id == event_id) else {
        return Err("Liability event not found".to_string());
    };

    let prev_linked_tx = vault.liability_events[ev_idx].linked_tx_id.clone();

    if ignore {
        let ev = &mut vault.liability_events[ev_idx];
        ev.status = LiabilityEventStatus::Ignored;
        ev.manual_locked = true;
        ev.linked_tx_id = None;
        ev.resolved_amount_minor = None;
        ev.resolved_currency = None;
        ev.scale = None;

        if let Some(prev_id) = prev_linked_tx {
            if let Some(tx) = vault.transactions.iter_mut().find(|t| t.id == prev_id) {
                if tx.linked_liability_event_id.as_deref() == Some(event_id) {
                    tx.linked_liability_event_id = None;
                    tx.linked_liability_source_id = None;
                    tx.linked_sub_account = None;
                    if tx.manual_classification == Some(TxClassification::DebtPayment) {
                        tx.manual_classification = None;
                    }
                }
            }
        }
    } else if let Some(tx_id) = linked_tx_id {
        let ev_id = vault.liability_events[ev_idx].id.clone();
        let ev_src = vault.liability_events[ev_idx].source_id.clone();
        let ev_sa = vault.liability_events[ev_idx].sub_account.clone();
        {
            let ev = &mut vault.liability_events[ev_idx];
            ev.status = LiabilityEventStatus::Linked;
            ev.manual_locked = true;
            ev.linked_tx_id = Some(tx_id.to_string());
            ev.resolved_amount_minor = linked_amount;
            ev.resolved_currency = linked_currency;
            ev.scale = linked_scale;
        }
        if let Some(tx) = vault.transactions.iter_mut().find(|t| t.id == tx_id) {
            tx.manual_classification = Some(TxClassification::DebtPayment);
            tx.classification = TxClassification::DebtPayment;
            tx.linked_liability_event_id = Some(ev_id);
            tx.linked_liability_source_id = Some(ev_src);
            tx.linked_sub_account = ev_sa;
        }
    } else {
        return Err("Either ignore=true or linked_tx_id must be provided".to_string());
    }

    recompute_matches(vault, window);
    let updated_ev = vault
        .liability_events
        .iter()
        .find(|e| e.id == event_id)
        .cloned()
        .ok_or_else(|| "Liability event not found".to_string())?;
    store.save_transactions_vault()?;
    Ok(LiabilityResolveResult { event: updated_ev })
}

pub fn apply_transaction_correction_in_vault(
    store: &mut VaultStore,
    tx_id: &str,
    classification: Option<TxClassification>,
    _category: Option<&str>,
    _apply_to_similar: bool,
) -> Result<CorrectionResult, String> {
    if !store.config().history_enabled {
        return Err("Notification storage is disabled".to_string());
    }
    if let Some(cls) = classification {
        if !matches!(
            cls,
            TxClassification::Transfer
                | TxClassification::DebtPayment
                | TxClassification::Normal
                | TxClassification::Ignored
        ) {
            return Err("Unsupported manual classification".to_string());
        }
    }

    store.ensure_loaded()?;
    let window = store.config().transfer_match_window_minutes;
    let vault = store.transactions_vault_mut();

    let Some(tx_idx) = vault.transactions.iter().position(|t| t.id == tx_id) else {
        return Err("Transaction not found".to_string());
    };

    let prev_pair_id = vault.transactions[tx_idx].transfer_pair_id.clone();
    let prev_ev_id = vault.transactions[tx_idx].linked_liability_event_id.clone();

    vault.transactions[tx_idx].manual_classification = classification;
    if let Some(cls) = classification {
        vault.transactions[tx_idx].classification = cls;
    }

    // Clear auto-pair/link metadata on this tx and any partner if overridden away from paired states
    vault.transactions[tx_idx].transfer_pair_id = None;
    vault.transactions[tx_idx].linked_liability_event_id = None;
    vault.transactions[tx_idx].linked_liability_source_id = None;
    vault.transactions[tx_idx].linked_sub_account = None;

    if let Some(pair_id) = prev_pair_id {
        for other in &mut vault.transactions {
            if other.id != tx_id && other.transfer_pair_id.as_deref() == Some(&pair_id) {
                other.transfer_pair_id = None;
            }
        }
    }
    if let Some(ev_id) = prev_ev_id {
        if let Some(ev) = vault.liability_events.iter_mut().find(|e| e.id == ev_id) {
            if ev.linked_tx_id.as_deref() == Some(tx_id) {
                ev.manual_locked = false;
                ev.linked_tx_id = None;
                ev.resolved_amount_minor = None;
                ev.resolved_currency = None;
                ev.scale = None;
                ev.status = LiabilityEventStatus::UnmatchedNeedsReview;
            }
        }
    }

    recompute_matches(vault, window);
    let updated_tx = vault
        .transactions
        .iter()
        .find(|t| t.id == tx_id)
        .cloned()
        .ok_or_else(|| "Transaction not found".to_string())?;
    store.save_transactions_vault()?;

    Ok(CorrectionResult {
        updated_count: 1,
        transaction: updated_tx,
    })
}

#[tauri::command]
pub fn phone_get_transactions(
    app: AppHandle,
    state: State<'_, PhoneHubState>,
    hw_serial: Option<String>,
    currency: Option<String>,
    from_ms: Option<i64>,
    to_ms: Option<i64>,
) -> Result<MoneyVaultSnapshot, String> {
    let mut store = state
        .notification_vault
        .lock()
        .map_err(crate::state::poison_error)?;
    ensure_vault_dir(&app, &mut store)?;
    build_money_snapshot(
        &mut store,
        hw_serial.as_deref(),
        currency.as_deref(),
        from_ms,
        to_ms,
    )
}

#[tauri::command]
pub fn phone_resolve_liability_event(
    app: AppHandle,
    state: State<'_, PhoneHubState>,
    event_id: String,
    linked_tx_id: Option<String>,
    ignore: bool,
) -> Result<LiabilityResolveResult, String> {
    let mut store = state
        .notification_vault
        .lock()
        .map_err(crate::state::poison_error)?;
    ensure_vault_dir(&app, &mut store)?;
    resolve_liability_event_in_vault(&mut store, &event_id, linked_tx_id.as_deref(), ignore)
}

#[tauri::command]
pub fn phone_apply_transaction_correction(
    app: AppHandle,
    state: State<'_, PhoneHubState>,
    tx_id: String,
    classification: Option<TxClassification>,
    category: Option<String>,
    apply_to_similar: Option<bool>,
) -> Result<CorrectionResult, String> {
    let mut store = state
        .notification_vault
        .lock()
        .map_err(crate::state::poison_error)?;
    ensure_vault_dir(&app, &mut store)?;
    apply_transaction_correction_in_vault(
        &mut store,
        &tx_id,
        classification,
        category.as_deref(),
        apply_to_similar.unwrap_or(false),
    )
}

// ============================================================================
// Local Real-Data Validation Harness (Aggregate Counts Only, Zero Leaks)
// ============================================================================

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceTypeHarnessCounts {
    pub parsed_in: usize,
    pub parsed_out: usize,
    pub bill_paid_emitted: usize,
    pub bill_paid_collapsed: usize,
    pub bill_paid_linked: usize,
    pub transfers_matched: usize,
    pub debt_payments: usize,
    pub possible_transfers: usize,
    pub amount_lines_unmatched: usize,
}

#[derive(Clone, Debug, Default)]
pub struct HarnessReport {
    pub total_records_deduped: usize,
    pub by_source_type: BTreeMap<String, SourceTypeHarnessCounts>,
    pub by_rule_id: BTreeMap<String, usize>,
}

fn source_type_key(st: MoneySourceType) -> &'static str {
    match st {
        MoneySourceType::Bank => "bank",
        MoneySourceType::Ewallet => "ewallet",
        MoneySourceType::Paylater => "paylater",
        MoneySourceType::CreditCard => "credit_card",
    }
}

pub fn parse_harness_sources_json(raw: &str) -> Result<NotificationStorageConfig, String> {
    let mut cfg: NotificationStorageConfig =
        serde_json::from_str(raw).map_err(|_| "Invalid sources JSON".to_string())?;
    cfg.history_enabled = true;
    cfg.transfer_match_window_minutes = cfg.transfer_match_window_minutes.clamp(1, 60);

    if cfg.per_app.is_empty() {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(raw) {
            if let Some(map) = val.get("apps").and_then(|v| v.as_object()) {
                for (pkg, pol_val) in map {
                    if let Ok(pol) =
                        serde_json::from_value::<AppNotificationPolicy>(pol_val.clone())
                    {
                        cfg.per_app.insert(pkg.clone(), pol);
                    }
                }
            }
        }
    }

    Ok(cfg)
}

pub fn run_capture_harness_from_dumpsys(
    captures: &[&str],
    sources_cfg: &NotificationStorageConfig,
    user_rules_json: Option<&str>,
) -> HarnessReport {
    let loaded_rules = load_rules_from_strings(DEFAULT_RULES_JSON, user_rules_json);
    let amount_token_re = Regex::new(
        r"(?i)(?:Rp\.?|IDR|USD|SGD|MYR|EUR|GBP|AUD|JPY|CNY|KRW|THB|VND|PHP|HKD|TWD|CHF|CAD|NZD|INR|\$)\s*[0-9][0-9.,]*",
    )
    .expect("valid amount detector regex");

    let mut seen_keys: HashSet<(String, i64)> = HashSet::new();
    let mut vault = TransactionsVault::default();
    let mut report = HarnessReport::default();

    for raw in captures {
        let records = parse_dumpsys_notifications_rich(raw);
        for rec in records {
            if !seen_keys.insert((rec.base.key.clone(), rec.base.posted_unix)) {
                continue;
            }
            report.total_records_deduped += 1;

            let Some(policy) = sources_cfg.per_app.get(&rec.base.package) else {
                continue;
            };
            if policy.retention == RetentionPolicy::None {
                continue;
            }
            let enabled_types: Vec<MoneySourceType> = policy
                .money_sources
                .iter()
                .filter(|s| s.enabled)
                .map(|s| s.source_type)
                .collect();
            if enabled_types.is_empty() {
                continue;
            }

            let notif = PhoneHubNotification {
                key: rec.base.key,
                package: rec.base.package,
                app_label: rec.base.app_label.unwrap_or_default(),
                title: rec.base.title,
                text: rec.base.text,
                posted_unix: rec.base.posted_unix,
            };

            match parse_notification_for_sources(
                &loaded_rules,
                "HARNESS",
                &notif,
                policy,
                rec.base.posted_unix,
                &vault.rule_salt,
            ) {
                Some(ParsedMoneyOutcome::Transaction(tx)) => {
                    *report.by_rule_id.entry(tx.rule_id.clone()).or_insert(0) += 1;
                    if !is_statement_due_record(&tx) {
                        let counts = report
                            .by_source_type
                            .entry(source_type_key(tx.source_type).to_string())
                            .or_default();
                        match tx.direction {
                            TxDirection::In => counts.parsed_in += 1,
                            TxDirection::Out => counts.parsed_out += 1,
                        }
                    }
                    vault.transactions.push(tx);
                }
                Some(ParsedMoneyOutcome::LiabilityEvent(ev)) => {
                    *report.by_rule_id.entry(ev.rule_id.clone()).or_insert(0) += 1;
                    let counts = report
                        .by_source_type
                        .entry(source_type_key(ev.source_type).to_string())
                        .or_default();
                    counts.bill_paid_emitted += 1;
                    vault.liability_events.push(ev);
                }
                None => {
                    let combined = format!("{}\n{}", notif.title, notif.text);
                    if amount_token_re.is_match(&combined) {
                        let mut counted = HashSet::new();
                        for st in enabled_types {
                            if counted.insert(source_type_key(st)) {
                                report
                                    .by_source_type
                                    .entry(source_type_key(st).to_string())
                                    .or_default()
                                    .amount_lines_unmatched += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    recompute_matches(&mut vault, sources_cfg.transfer_match_window_minutes);

    for tx in &vault.transactions {
        if is_statement_due_record(tx) {
            continue;
        }
        let counts = report
            .by_source_type
            .entry(source_type_key(tx.source_type).to_string())
            .or_default();
        match tx.classification {
            TxClassification::Transfer => counts.transfers_matched += 1,
            TxClassification::DebtPayment => counts.debt_payments += 1,
            TxClassification::PossibleTransfer => counts.possible_transfers += 1,
            TxClassification::Normal
            | TxClassification::RepaymentReceived
            | TxClassification::Ignored => {}
        }
    }

    for ev in &vault.liability_events {
        let counts = report
            .by_source_type
            .entry(source_type_key(ev.source_type).to_string())
            .or_default();
        if ev.duplicate_of.is_some() {
            counts.bill_paid_collapsed += 1;
        } else if ev.status == LiabilityEventStatus::Linked {
            counts.bill_paid_linked += 1;
        }
    }

    report
}

pub fn print_harness_report(report: &HarnessReport) {
    println!("total_records_deduped={}", report.total_records_deduped);
    for (st, c) in &report.by_source_type {
        println!(
            "source_type={} parsed_in={} parsed_out={} bill_paid_emitted={} bill_paid_collapsed={} bill_paid_linked={} transfers_matched={} debt_payments={} possible_transfers={} amount_lines_unmatched={}",
            st,
            c.parsed_in,
            c.parsed_out,
            c.bill_paid_emitted,
            c.bill_paid_collapsed,
            c.bill_paid_linked,
            c.transfers_matched,
            c.debt_payments,
            c.possible_transfers,
            c.amount_lines_unmatched
        );
    }
    for (rule_id, count) in &report.by_rule_id {
        println!("rule_id={} count={}", rule_id, count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phone_hub::vault::{
        dpapi_protect_with_entropy, dpapi_unprotect_with_entropy, HISTORY_VAULT_FILENAME,
        TRANSACTIONS_VAULT_FILENAME,
    };
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQ: AtomicU64 = AtomicU64::new(1);

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "anyleap_money_test_{tag}_{}_{}",
            std::process::id(),
            TEST_SEQ.fetch_add(1, Ordering::Relaxed)
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

    fn policy_with_sources(
        retention: RetentionPolicy,
        sources: Vec<MoneySourceConfig>,
    ) -> AppNotificationPolicy {
        AppNotificationPolicy {
            retention,
            retention_days: 30,
            money_sources: sources,
            sensitive: true,
        }
    }

    fn src(id: &str, source_type: MoneySourceType, rule_template: &str) -> MoneySourceConfig {
        MoneySourceConfig {
            id: id.to_string(),
            source_type,
            rule_template: rule_template.to_string(),
            enabled: true,
            label: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn make_tx(
        id: &str,
        pkg: &str,
        source_id: &str,
        source_type: MoneySourceType,
        direction: TxDirection,
        amount_minor: i64,
        currency: &str,
        timestamp_ms: i64,
    ) -> StoredTransaction {
        StoredTransaction {
            id: id.to_string(),
            dedupe_id: format!("HW1::{id}::1"),
            hw_serial: "HW1".to_string(),
            package: pkg.to_string(),
            source_id: source_id.to_string(),
            app_label: "Example".to_string(),
            source_type,
            sub_account: None,
            direction,
            amount_minor,
            scale: 2,
            currency: currency.to_string(),
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
            timestamp_ms,
            rule_id: "r_test".to_string(),
        }
    }

    fn make_bill_paid(
        id: &str,
        pkg: &str,
        source_id: &str,
        source_type: MoneySourceType,
        sub_account: Option<&str>,
        timestamp_ms: i64,
    ) -> LiabilityEvent {
        LiabilityEvent {
            id: id.to_string(),
            dedupe_id: format!("HW1::{id}::1"),
            hw_serial: "HW1".to_string(),
            package: pkg.to_string(),
            source_id: source_id.to_string(),
            app_label: "Example Liability".to_string(),
            source_type,
            event_type: LiabilityEventType::BillPaid,
            sub_account: sub_account.map(str::to_string),
            status: LiabilityEventStatus::UnmatchedNeedsReview,
            linked_tx_id: None,
            resolved_amount_minor: None,
            resolved_currency: None,
            scale: None,
            manual_locked: false,
            duplicate_of: None,
            timestamp_ms,
            rule_id: "r_bill".to_string(),
        }
    }

    #[test]
    fn parse_localized_amount_handles_all_formats_and_rejects_invalid() {
        assert_eq!(
            parse_localized_amount("1,021,403.00", Some("IDR"), NumberFormat::Auto),
            Some((102_140_300, 2))
        );
        assert_eq!(
            parse_localized_amount("1,021,403.00", Some("USD"), NumberFormat::DotDecimal),
            Some((102_140_300, 2))
        );
        assert_eq!(
            parse_localized_amount("1.021.403,00", Some("IDR"), NumberFormat::Auto),
            Some((102_140_300, 2))
        );
        assert_eq!(
            parse_localized_amount("1.021.403,00", Some("IDR"), NumberFormat::CommaDecimal),
            Some((102_140_300, 2))
        );
        assert_eq!(
            parse_localized_amount("45.000", Some("IDR"), NumberFormat::Auto),
            Some((4_500_000, 2))
        );
        assert_eq!(
            parse_localized_amount("45.000", None, NumberFormat::CommaDecimal),
            Some((4_500_000, 2))
        );
        assert_eq!(
            parse_localized_amount("120000", Some("IDR"), NumberFormat::Auto),
            Some((12_000_000, 2))
        );
        assert_eq!(
            parse_localized_amount("12.50", Some("USD"), NumberFormat::Auto),
            Some((1_250, 2))
        );
        assert_eq!(
            parse_localized_amount("12,50", Some("EUR"), NumberFormat::Auto),
            Some((1_250, 2))
        );
        assert_eq!(
            parse_localized_amount("-500", Some("IDR"), NumberFormat::Auto),
            None
        );
        assert_eq!(
            parse_localized_amount("12rb", Some("IDR"), NumberFormat::Auto),
            None
        );
        assert_eq!(
            parse_localized_amount("1..23", Some("IDR"), NumberFormat::Auto),
            None
        );
    }

    #[test]
    fn parser_covers_all_14_shapes_with_generic_templates() {
        let rules = load_rules_from_strings(DEFAULT_RULES_JSON, None);
        let salt = [7u8; 32];

        // Shape 1: Bank OUT with category after "at"
        let bank_policy = policy_with_sources(
            RetentionPolicy::Transactions,
            vec![src("bank_1", MoneySourceType::Bank, "generic_bank")],
        );
        let n1 = make_notif(
            "com.example.bank",
            "k1",
            "Financial Diary",
            "You spent IDR 1,021,403.00 at Groceries.",
            1_700_000_001,
        );
        let r1 =
            parse_notification_for_sources(&rules, "HW1", &n1, &bank_policy, 1_700_000_001, &salt);
        let Some(ParsedMoneyOutcome::Transaction(tx1)) = r1 else {
            panic!("Shape 1 should parse as transaction");
        };
        assert_eq!(tx1.direction, TxDirection::Out);
        assert_eq!(tx1.amount_minor, 102_140_300);
        assert_eq!(tx1.currency, "IDR");
        assert_eq!(tx1.category, "Groceries");

        // Shape 2: Bank IN with masked sender hashed & dropped and category after "at"
        let n2 = make_notif(
            "com.example.bank",
            "k2",
            "Financial Diary",
            "You received IDR 2,500,000.00 from J*** D*** at Income.",
            1_700_000_002,
        );
        let r2 =
            parse_notification_for_sources(&rules, "HW1", &n2, &bank_policy, 1_700_000_002, &salt);
        let Some(ParsedMoneyOutcome::Transaction(tx2)) = r2 else {
            panic!("Shape 2 should parse as transaction");
        };
        assert_eq!(tx2.direction, TxDirection::In);
        assert_eq!(tx2.amount_minor, 250_000_000);
        assert_eq!(tx2.category, "Income");
        assert!(tx2.similarity_hash.is_some());
        let json2 = serde_json::to_string(&tx2).unwrap();
        assert!(!json2.contains("J***"));

        // Shapes 3..6: Shopping app using ONLY generic_credit_card template
        let shop_cc_policy = policy_with_sources(
            RetentionPolicy::Transactions,
            vec![src(
                "shop_cc",
                MoneySourceType::CreditCard,
                "generic_credit_card",
            )],
        );
        let n3 = make_notif(
            "com.example.shop",
            "k3",
            "Transaksi Kartu Kredit Sample Card",
            "Kamu baru transaksi pakai Sample Card. Transaksi senilai Rp 245.500,00 di Sample Roastery 25/11. Untuk Alex, klik di Sample Roastery.",
            1_700_000_003,
        );
        let r3 = parse_notification_for_sources(
            &rules,
            "HW1",
            &n3,
            &shop_cc_policy,
            1_700_000_003,
            &salt,
        );
        let Some(ParsedMoneyOutcome::Transaction(tx3)) = r3 else {
            panic!("Shape 3 should parse as credit_card OUT");
        };
        assert_eq!(tx3.source_type, MoneySourceType::CreditCard);
        assert_eq!(tx3.direction, TxDirection::Out);
        assert_eq!(tx3.amount_minor, 24_550_000);

        // Shape 4: credit card bill paid (amount-less) with cardLast4 hashed into sub_account
        let n4 = make_notif(
            "com.example.shop",
            "k4",
            "Transaksi Pembayaran Tagihan Kartu Kredit Anda Berhasil",
            "Pembayaran tagihan kartu kredit Sample Card untuk Alex: 4111-22**-****-0042 Anda BERHASIL pada tanggal 25 Nov",
            1_700_000_004,
        );
        let r4 = parse_notification_for_sources(
            &rules,
            "HW1",
            &n4,
            &shop_cc_policy,
            1_700_000_004,
            &salt,
        );
        let Some(ParsedMoneyOutcome::LiabilityEvent(ev4)) = r4 else {
            panic!("Shape 4 should parse as bill_paid");
        };
        assert_eq!(ev4.event_type, LiabilityEventType::BillPaid);
        assert!(ev4
            .sub_account
            .as_deref()
            .is_some_and(|s| s.starts_with("sa_")));
        let json4 = serde_json::to_string(&ev4).unwrap();
        assert!(!json4.contains("0042"));
        assert!(!json4.contains("4111"));

        // Shape 5: free shipping promo -> NOTHING
        let n5 = make_notif(
            "com.example.shop",
            "k5",
            "Misi kaak Alex",
            "Ada Gratis Ongkir juga untuk Alex 25/11...~ Udah liat promonya?",
            1_700_000_005,
        );
        assert_eq!(
            parse_notification_for_sources(
                &rules,
                "HW1",
                &n5,
                &shop_cc_policy,
                1_700_000_005,
                &salt
            ),
            None
        );

        // Shape 6: "12rb" promo -> NOTHING
        let n6 = make_notif(
            "com.example.shop",
            "k6",
            "👖 Sis, Baggy Jeans CUMA 12rb",
            "Atasan wanita juga lagi diskon di Sample Store Gajian Sale November 🤑",
            1_700_000_006,
        );
        assert_eq!(
            parse_notification_for_sources(
                &rules,
                "HW1",
                &n6,
                &shop_cc_policy,
                1_700_000_006,
                &salt
            ),
            None
        );

        // Shapes 7..13: E-wallet app with generic_ewallet + generic_paylater sources
        let wallet_policy = policy_with_sources(
            RetentionPolicy::Transactions,
            vec![
                src("ew_main", MoneySourceType::Ewallet, "generic_ewallet"),
                src("pl_main", MoneySourceType::Paylater, "generic_paylater"),
            ],
        );

        // Shape 7: ewallet OUT with transferHint: Repayment -> DebtPayment even unpaired
        let n7 = make_notif(
            "com.example.wallet1",
            "k7",
            "Uang terkirim! 🤝",
            "Kamu berhasil bayar sebesar Rp 150.000 buat InstallmentService",
            1_700_000_007,
        );
        let r7 = parse_notification_for_sources(
            &rules,
            "HW1",
            &n7,
            &wallet_policy,
            1_700_000_007,
            &salt,
        );
        let Some(ParsedMoneyOutcome::Transaction(tx7)) = r7 else {
            panic!("Shape 7 should parse as ewallet OUT");
        };
        assert_eq!(tx7.source_type, MoneySourceType::Ewallet);
        assert_eq!(tx7.direction, TxDirection::Out);
        assert_eq!(tx7.amount_minor, 15_000_000);
        assert_eq!(tx7.transfer_hint, Some(TransferHint::Repayment));
        assert_eq!(tx7.classification, TxClassification::DebtPayment);

        // Shape 8: paylater bill_paid (limit restored)
        let n8 = make_notif(
            "com.example.wallet1",
            "k8",
            "Yey! Pembayaran InstallmentService berhasil ✨",
            "Limit InstallmentService kamu juga udah keisi lagi, ya. Klik buat cek rinciannya.",
            1_700_000_008,
        );
        let r8 = parse_notification_for_sources(
            &rules,
            "HW1",
            &n8,
            &wallet_policy,
            1_700_000_008,
            &salt,
        );
        let Some(ParsedMoneyOutcome::LiabilityEvent(ev8)) = r8 else {
            panic!("Shape 8 should parse as paylater bill_paid");
        };
        assert_eq!(ev8.source_type, MoneySourceType::Paylater);
        assert_eq!(ev8.event_type, LiabilityEventType::BillPaid);

        // Shape 9: paylater bill_paid (tagihan berhasil + limit keisi)
        let n9 = make_notif(
            "com.example.wallet1",
            "k9",
            "Pembayaran tagihan berhasil ✅",
            "Limit InstallmentService kamu juga udah keisi lagi, ya. Horee! Klik buat cek rinciannya.",
            1_700_000_009,
        );
        let r9 = parse_notification_for_sources(
            &rules,
            "HW1",
            &n9,
            &wallet_policy,
            1_700_000_009,
            &salt,
        );
        let Some(ParsedMoneyOutcome::LiabilityEvent(ev9)) = r9 else {
            panic!("Shape 9 should parse as paylater bill_paid");
        };
        assert_eq!(ev9.source_type, MoneySourceType::Paylater);

        // Shape 10: utility payment without amount -> NOTHING (must NOT become bill_paid)
        let n10 = make_notif(
            "com.example.wallet1",
            "k10",
            "Yes, sukses bayar!",
            "Pembayaran Listrik Pascabayar kamu berhasil dibayar! Tos dulu, dong~",
            1_700_000_010,
        );
        assert_eq!(
            parse_notification_for_sources(
                &rules,
                "HW1",
                &n10,
                &wallet_policy,
                1_700_000_010,
                &salt
            ),
            None
        );

        // Shape 11: unfinished -> NOTHING
        let n11 = make_notif(
            "com.example.wallet1",
            "k11",
            "Hi Alex, lanjutin transaksi kamu yuk!",
            "Transaksi Token Listrik kamu dikit lagi selesai. Klik buat lanjutin.",
            1_700_000_011,
        );
        assert_eq!(
            parse_notification_for_sources(
                &rules,
                "HW1",
                &n11,
                &wallet_policy,
                1_700_000_011,
                &salt
            ),
            None
        );

        // Shape 12: unpaid VA reminder -> NOTHING
        let n12 = make_notif(
            "com.example.wallet1",
            "k12",
            "Selesaikan pembayaran VA sekarang, ya 🔔",
            "Nomor VA-nya berlaku selama 25 Nov. Silakan transfer Rp 120.000 ke Alex kirim ke Alex",
            1_700_000_012,
        );
        assert_eq!(
            parse_notification_for_sources(
                &rules,
                "HW1",
                &n12,
                &wallet_policy,
                1_700_000_012,
                &salt
            ),
            None
        );

        // Shape 13: reward coins (no currency) -> NOTHING
        let n13 = make_notif(
            "com.example.wallet1",
            "k13",
            "Criiing! Kamu dapet Loyalty Coins 💰",
            "Cihuy, kamu dapet 1.500 Loyalty Coins! Klik buat liat detailnya.",
            1_700_000_013,
        );
        assert_eq!(
            parse_notification_for_sources(
                &rules,
                "HW1",
                &n13,
                &wallet_policy,
                1_700_000_013,
                &salt
            ),
            None
        );

        // Shape 14: second e-wallet paylater bill_paid
        let wallet2_policy = policy_with_sources(
            RetentionPolicy::Transactions,
            vec![src("pl_2", MoneySourceType::Paylater, "generic_paylater")],
        );
        let n14 = make_notif(
            "com.example.wallet2",
            "k14",
            "DeferredPay",
            "Selamat! Pembayaran tagihan DeferredPay kamu telah diterima dan limit DeferredPay kamu akan segera dikembalikan.",
            1_700_000_014,
        );
        let r14 = parse_notification_for_sources(
            &rules,
            "HW1",
            &n14,
            &wallet2_policy,
            1_700_000_014,
            &salt,
        );
        let Some(ParsedMoneyOutcome::LiabilityEvent(ev14)) = r14 else {
            panic!("Shape 14 should parse as paylater bill_paid");
        };
        assert_eq!(ev14.source_type, MoneySourceType::Paylater);

        // Email app (zero money sources) -> NOTHING
        let email_policy = policy_with_sources(RetentionPolicy::History, vec![]);
        let n_email = make_notif(
            "com.example.mail",
            "k15",
            "Receipt IDR 250,000.00",
            "You spent IDR 250,000.00 at Store.",
            1_700_000_015,
        );
        assert_eq!(
            parse_notification_for_sources(
                &rules,
                "HW1",
                &n_email,
                &email_policy,
                1_700_000_015,
                &salt
            ),
            None
        );
    }

    #[test]
    fn parser_supports_spec_54_fixtures_two_cards_and_user_rules_priority() {
        let user_json = r#"{
            "version": 1,
            "rules": [
                {
                    "id": "broken-regex-rule",
                    "enabled": true,
                    "priority": 500,
                    "package": "com.example.bank",
                    "ruleTemplate": "*",
                    "sourceTypes": ["bank"],
                    "eventKind": "transaction",
                    "direction": "out",
                    "pattern": "[unclosed("
                },
                {
                    "id": "custom-bank-pkg-override",
                    "enabled": true,
                    "priority": 10,
                    "package": "com.example.bank",
                    "ruleTemplate": "*",
                    "sourceTypes": ["bank"],
                    "eventKind": "transaction",
                    "direction": "out",
                    "pattern": "(?i)you spent\\s+(?P<currency>IDR)\\s*(?P<amount>[0-9.,]+)\\s+at\\s+(?P<category>[A-Za-z ]+)",
                    "defaultCategory": "CustomOverride"
                }
            ]
        }"#;
        let rules = load_rules_from_strings(DEFAULT_RULES_JSON, Some(user_json));
        assert!(rules.warnings.contains(&"broken-regex-rule".to_string()));

        let salt = [9u8; 32];
        let bank_policy = policy_with_sources(
            RetentionPolicy::Transactions,
            vec![src("b1", MoneySourceType::Bank, "generic_bank")],
        );
        let n_override = make_notif(
            "com.example.bank",
            "k_ov",
            "Alert",
            "You spent IDR 120,000.00 at Groceries.",
            1_700_000_100,
        );
        let Some(ParsedMoneyOutcome::Transaction(tx_ov)) = parse_notification_for_sources(
            &rules,
            "HW1",
            &n_override,
            &bank_policy,
            1_700_000_100,
            &salt,
        ) else {
            panic!("expected override match");
        };
        // Package specificity beats generic "*" even with lower priority
        assert_eq!(tx_ov.rule_id, "custom-bank-pkg-override");

        // Now test defaults without override for all §5.4 fixtures
        let def_rules = load_rules_from_strings(DEFAULT_RULES_JSON, None);
        let fixtures = [
            (
                "Pembayaran Rp 45.000 di Makan",
                TxDirection::Out,
                4_500_000,
                None,
            ),
            (
                "Transaksi debit IDR 1.021.403,00 di Belanja",
                TxDirection::Out,
                102_140_300,
                None,
            ),
            (
                "Payment OUT Rp 245.500,00 — Card Bill",
                TxDirection::Out,
                24_550_000,
                Some(TransferHint::Payment),
            ),
            (
                "Transfer ke Virtual Account Rp 250.000",
                TxDirection::Out,
                25_000_000,
                Some(TransferHint::VirtualAccount),
            ),
            (
                "Top up e-wallet Rp 100.000",
                TxDirection::Out,
                10_000_000,
                Some(TransferHint::TopUp),
            ),
            (
                "Transfer masuk Rp 1.021.403,00",
                TxDirection::In,
                102_140_300,
                None,
            ),
            (
                "Dana masuk IDR 500,000.00",
                TxDirection::In,
                50_000_000,
                None,
            ),
        ];
        for (i, (text, exp_dir, exp_amt, exp_hint)) in fixtures.iter().enumerate() {
            let n = make_notif(
                "com.example.bank",
                &format!("fx_{i}"),
                "",
                text,
                1_700_000_200 + i as i64,
            );
            let Some(ParsedMoneyOutcome::Transaction(tx)) = parse_notification_for_sources(
                &def_rules,
                "HW1",
                &n,
                &bank_policy,
                1_700_000_200,
                &salt,
            ) else {
                panic!("fixture {i} should parse");
            };
            assert_eq!(tx.direction, *exp_dir);
            assert_eq!(tx.amount_minor, *exp_amt);
            assert_eq!(tx.transfer_hint, *exp_hint);
        }

        // Cobranded shopping app promo bleed guard + two cards separation
        let cobranded_policy = policy_with_sources(
            RetentionPolicy::Transactions,
            vec![src(
                "shop_cc",
                MoneySourceType::CreditCard,
                "cobranded_card_in_shop_app",
            )],
        );
        let promo = make_notif(
            "com.example.shop",
            "p1",
            "Flash Deal!",
            "Flash Deal! Wireless Mouse Rp 245.500,00 today only",
            1_700_000_300,
        );
        assert_eq!(
            parse_notification_for_sources(
                &def_rules,
                "HW1",
                &promo,
                &cobranded_policy,
                1_700_000_300,
                &salt
            ),
            None
        );

        let card_a = make_notif(
            "com.example.shop",
            "ca",
            "Bill Paid",
            "Credit card bill payment for card 4111-22**-****-0042 succeeded on 25 Nov",
            1_700_000_301,
        );
        let card_b = make_notif(
            "com.example.shop",
            "cb",
            "Bill Paid",
            "Credit card bill payment for card 4111-22**-****-9012 succeeded on 25 Nov",
            1_700_000_302,
        );
        let Some(ParsedMoneyOutcome::LiabilityEvent(ev_a)) = parse_notification_for_sources(
            &def_rules,
            "HW1",
            &card_a,
            &cobranded_policy,
            1_700_000_301,
            &salt,
        ) else {
            panic!("card A bill_paid expected");
        };
        let Some(ParsedMoneyOutcome::LiabilityEvent(ev_b)) = parse_notification_for_sources(
            &def_rules,
            "HW1",
            &card_b,
            &cobranded_policy,
            1_700_000_302,
            &salt,
        ) else {
            panic!("card B bill_paid expected");
        };
        assert!(ev_a.sub_account.is_some());
        assert!(ev_b.sub_account.is_some());
        assert_ne!(ev_a.sub_account, ev_b.sub_account);

        let mut vault = TransactionsVault {
            rule_salt: salt,
            liability_events: vec![ev_a, ev_b],
            ..Default::default()
        };
        let serialized = serde_json::to_string(&vault).unwrap();
        assert!(!serialized.contains("4111"));
        assert!(!serialized.contains("0042"));
        assert!(!serialized.contains("9012"));

        // Statement due fixture does not double count as spending
        let due_notif = make_notif(
            "com.example.shop",
            "due1",
            "Statement",
            "Tagihan jatuh tempo IDR 1.500.000,00",
            1_700_000_303,
        );
        let Some(ParsedMoneyOutcome::Transaction(due_tx)) = parse_notification_for_sources(
            &def_rules,
            "HW1",
            &due_notif,
            &cobranded_policy,
            1_700_000_303,
            &salt,
        ) else {
            panic!("statement due expected");
        };
        assert_eq!(due_tx.statement_due_minor, Some(150_000_000));
        assert_eq!(due_tx.amount_minor, 0);
        assert_eq!(due_tx.classification, TxClassification::Ignored);
        vault.transactions.push(due_tx);
        recompute_matches(&mut vault, 15);
        assert_eq!(
            vault.transactions[0].classification,
            TxClassification::Ignored
        );
    }

    #[test]
    fn matcher_covers_all_12_spec_cases_plus_collapse_and_repayment_cases() {
        let base_ts = 1_700_000_000_000_i64;
        let min_ms = 60_000_i64;

        // 1. Exact match within +-15 min (bank OUT + ewallet IN) -> Transfer
        let mut v1 = TransactionsVault {
            transactions: vec![
                make_tx(
                    "out1",
                    "com.example.bank",
                    "b1",
                    MoneySourceType::Bank,
                    TxDirection::Out,
                    10_000_000,
                    "IDR",
                    base_ts,
                ),
                make_tx(
                    "in1",
                    "com.example.ew",
                    "e1",
                    MoneySourceType::Ewallet,
                    TxDirection::In,
                    10_000_000,
                    "IDR",
                    base_ts + 5 * min_ms,
                ),
            ],
            ..Default::default()
        };
        recompute_matches(&mut v1, 15);
        assert_eq!(
            v1.transactions[0].classification,
            TxClassification::Transfer
        );
        assert_eq!(
            v1.transactions[1].classification,
            TxClassification::Transfer
        );
        assert_eq!(
            v1.transactions[0].transfer_pair_id,
            v1.transactions[1].transfer_pair_id
        );
        assert!(v1.transactions[0].transfer_pair_id.is_some());

        // 2. Outside configurable window (15m 01s at 15m -> unmatched; at 20m -> matched)
        let mut v2 = TransactionsVault {
            transactions: vec![
                make_tx(
                    "out1",
                    "com.example.bank",
                    "b1",
                    MoneySourceType::Bank,
                    TxDirection::Out,
                    10_000_000,
                    "IDR",
                    base_ts,
                ),
                make_tx(
                    "in1",
                    "com.example.ew",
                    "e1",
                    MoneySourceType::Ewallet,
                    TxDirection::In,
                    10_000_000,
                    "IDR",
                    base_ts + 15 * min_ms + 1_000,
                ),
            ],
            ..Default::default()
        };
        recompute_matches(&mut v2, 15);
        assert_eq!(v2.transactions[0].classification, TxClassification::Normal);
        recompute_matches(&mut v2, 20);
        assert_eq!(
            v2.transactions[0].classification,
            TxClassification::Transfer
        );

        // 3. Currency mismatch -> not matched
        let mut v3 = TransactionsVault {
            transactions: vec![
                make_tx(
                    "out1",
                    "com.example.bank",
                    "b1",
                    MoneySourceType::Bank,
                    TxDirection::Out,
                    10_000_000,
                    "IDR",
                    base_ts,
                ),
                make_tx(
                    "in1",
                    "com.example.ew",
                    "e1",
                    MoneySourceType::Ewallet,
                    TxDirection::In,
                    10_000_000,
                    "USD",
                    base_ts + min_ms,
                ),
            ],
            ..Default::default()
        };
        recompute_matches(&mut v3, 15);
        assert_eq!(v3.transactions[0].classification, TxClassification::Normal);

        // 4. Same source (package, source_id) -> not matched; distinct source_ids inside same package -> matched
        let mut v4_same = TransactionsVault {
            transactions: vec![
                make_tx(
                    "out1",
                    "com.example.bank",
                    "b1",
                    MoneySourceType::Bank,
                    TxDirection::Out,
                    10_000_000,
                    "IDR",
                    base_ts,
                ),
                make_tx(
                    "in1",
                    "com.example.bank",
                    "b1",
                    MoneySourceType::Bank,
                    TxDirection::In,
                    10_000_000,
                    "IDR",
                    base_ts + min_ms,
                ),
            ],
            ..Default::default()
        };
        recompute_matches(&mut v4_same, 15);
        assert_eq!(
            v4_same.transactions[0].classification,
            TxClassification::Normal
        );

        let mut v4_diff = TransactionsVault {
            transactions: vec![
                make_tx(
                    "out1",
                    "com.example.bank",
                    "b1",
                    MoneySourceType::Bank,
                    TxDirection::Out,
                    10_000_000,
                    "IDR",
                    base_ts,
                ),
                make_tx(
                    "in1",
                    "com.example.bank",
                    "b2",
                    MoneySourceType::Ewallet,
                    TxDirection::In,
                    10_000_000,
                    "IDR",
                    base_ts + min_ms,
                ),
            ],
            ..Default::default()
        };
        recompute_matches(&mut v4_diff, 15);
        assert_eq!(
            v4_diff.transactions[0].classification,
            TxClassification::Transfer
        );

        // 5. Greedy closest-time 1:1
        let mut v5 = TransactionsVault {
            transactions: vec![
                make_tx(
                    "out_early",
                    "com.example.bank",
                    "b1",
                    MoneySourceType::Bank,
                    TxDirection::Out,
                    5_000_000,
                    "IDR",
                    base_ts,
                ),
                make_tx(
                    "out_close",
                    "com.example.bank",
                    "b1",
                    MoneySourceType::Bank,
                    TxDirection::Out,
                    5_000_000,
                    "IDR",
                    base_ts + 8 * min_ms,
                ),
                make_tx(
                    "in_1",
                    "com.example.ew",
                    "e1",
                    MoneySourceType::Ewallet,
                    TxDirection::In,
                    5_000_000,
                    "IDR",
                    base_ts + 9 * min_ms,
                ),
            ],
            ..Default::default()
        };
        recompute_matches(&mut v5, 15);
        assert_eq!(v5.transactions[0].classification, TxClassification::Normal);
        assert_eq!(
            v5.transactions[1].classification,
            TxClassification::Transfer
        );
        assert_eq!(
            v5.transactions[2].classification,
            TxClassification::Transfer
        );

        // 6. Equal |dt| tie-break prefers in >= out
        let mut v6 = TransactionsVault {
            transactions: vec![
                make_tx(
                    "out1",
                    "com.example.bank",
                    "b1",
                    MoneySourceType::Bank,
                    TxDirection::Out,
                    5_000_000,
                    "IDR",
                    base_ts,
                ),
                make_tx(
                    "in_before",
                    "com.example.ew",
                    "e1",
                    MoneySourceType::Ewallet,
                    TxDirection::In,
                    5_000_000,
                    "IDR",
                    base_ts - 5_000,
                ),
                make_tx(
                    "in_after",
                    "com.example.ew",
                    "e1",
                    MoneySourceType::Ewallet,
                    TxDirection::In,
                    5_000_000,
                    "IDR",
                    base_ts + 5_000,
                ),
            ],
            ..Default::default()
        };
        recompute_matches(&mut v6, 15);
        assert_eq!(v6.transactions[1].classification, TxClassification::Normal);
        assert_eq!(
            v6.transactions[2].classification,
            TxClassification::Transfer
        );

        // 7. Unmatched bank OUT with VA/top-up hint -> PossibleTransfer
        let mut tx_va = make_tx(
            "out_va",
            "com.example.bank",
            "b1",
            MoneySourceType::Bank,
            TxDirection::Out,
            5_000_000,
            "IDR",
            base_ts,
        );
        tx_va.transfer_hint = Some(TransferHint::VirtualAccount);
        let mut v7 = TransactionsVault {
            transactions: vec![
                tx_va,
                make_tx(
                    "out_norm",
                    "com.example.bank",
                    "b1",
                    MoneySourceType::Bank,
                    TxDirection::Out,
                    5_000_000,
                    "IDR",
                    base_ts,
                ),
            ],
            ..Default::default()
        };
        recompute_matches(&mut v7, 15);
        assert_eq!(
            v7.transactions[0].classification,
            TxClassification::PossibleTransfer
        );
        assert_eq!(v7.transactions[1].classification, TxClassification::Normal);

        // 8. Asset OUT + amounted Liability IN -> DebtPayment + RepaymentReceived
        let mut v8 = TransactionsVault {
            transactions: vec![
                make_tx(
                    "out1",
                    "com.example.bank",
                    "b1",
                    MoneySourceType::Bank,
                    TxDirection::Out,
                    20_000_000,
                    "IDR",
                    base_ts,
                ),
                make_tx(
                    "in_cc",
                    "com.example.shop",
                    "cc1",
                    MoneySourceType::CreditCard,
                    TxDirection::In,
                    20_000_000,
                    "IDR",
                    base_ts + 2 * min_ms,
                ),
            ],
            ..Default::default()
        };
        recompute_matches(&mut v8, 15);
        assert_eq!(
            v8.transactions[0].classification,
            TxClassification::DebtPayment
        );
        assert_eq!(
            v8.transactions[1].classification,
            TxClassification::RepaymentReceived
        );

        // 9. Manual override locks classification against re-matching
        let mut tx_locked = make_tx(
            "out1",
            "com.example.bank",
            "b1",
            MoneySourceType::Bank,
            TxDirection::Out,
            10_000_000,
            "IDR",
            base_ts,
        );
        tx_locked.manual_classification = Some(TxClassification::Normal);
        let mut v9 = TransactionsVault {
            transactions: vec![
                tx_locked,
                make_tx(
                    "in1",
                    "com.example.ew",
                    "e1",
                    MoneySourceType::Ewallet,
                    TxDirection::In,
                    10_000_000,
                    "IDR",
                    base_ts + min_ms,
                ),
            ],
            ..Default::default()
        };
        recompute_matches(&mut v9, 15);
        assert_eq!(v9.transactions[0].classification, TxClassification::Normal);
        assert_eq!(v9.transactions[1].classification, TxClassification::Normal);

        // 10. Bill paid with matching bank OUT (prefers Payment hint over closer generic OUT; closest among Payment OUTs)
        let mut tx_pay_far = make_tx(
            "pay_far",
            "com.example.bank",
            "b1",
            MoneySourceType::Bank,
            TxDirection::Out,
            24_550_000,
            "IDR",
            base_ts + min_ms,
        );
        tx_pay_far.transfer_hint = Some(TransferHint::Payment);
        let mut tx_pay_close = make_tx(
            "pay_close",
            "com.example.bank",
            "b1",
            MoneySourceType::Bank,
            TxDirection::Out,
            24_550_000,
            "IDR",
            base_ts + 3 * min_ms,
        );
        tx_pay_close.transfer_hint = Some(TransferHint::Payment);
        let tx_generic_closest = make_tx(
            "gen_closest",
            "com.example.bank",
            "b1",
            MoneySourceType::Bank,
            TxDirection::Out,
            12_000_000,
            "IDR",
            base_ts + 4 * min_ms,
        );
        let mut v10 = TransactionsVault {
            transactions: vec![tx_pay_far, tx_pay_close, tx_generic_closest],
            liability_events: vec![make_bill_paid(
                "ev1",
                "com.example.shop",
                "cc1",
                MoneySourceType::CreditCard,
                Some("sa_key_a"),
                base_ts + 5 * min_ms,
            )],
            ..Default::default()
        };
        recompute_matches(&mut v10, 15);
        assert_eq!(v10.transactions[0].classification, TxClassification::Normal);
        assert_eq!(
            v10.transactions[1].classification,
            TxClassification::DebtPayment
        );
        assert_eq!(
            v10.transactions[1].linked_sub_account.as_deref(),
            Some("sa_key_a")
        );
        assert_eq!(v10.transactions[2].classification, TxClassification::Normal);
        assert_eq!(v10.liability_events[0].status, LiabilityEventStatus::Linked);
        assert_eq!(
            v10.liability_events[0].linked_tx_id.as_deref(),
            Some("pay_close")
        );
        assert_eq!(
            v10.liability_events[0].resolved_amount_minor,
            Some(24_550_000)
        );

        // 11. Bill paid with no match -> UnmatchedNeedsReview
        let mut v11 = TransactionsVault {
            transactions: vec![make_tx(
                "old_out",
                "com.example.bank",
                "b1",
                MoneySourceType::Bank,
                TxDirection::Out,
                24_550_000,
                "IDR",
                base_ts,
            )],
            liability_events: vec![make_bill_paid(
                "ev1",
                "com.example.shop",
                "cc1",
                MoneySourceType::CreditCard,
                None,
                base_ts + 25 * min_ms,
            )],
            ..Default::default()
        };
        recompute_matches(&mut v11, 15);
        assert_eq!(
            v11.liability_events[0].status,
            LiabilityEventStatus::UnmatchedNeedsReview
        );

        // 12. Precedence: Pass 1 Transfer wins over Pass 2 bill_paid
        let mut tx_both = make_tx(
            "out_both",
            "com.example.bank",
            "b1",
            MoneySourceType::Bank,
            TxDirection::Out,
            50_000_000,
            "IDR",
            base_ts,
        );
        tx_both.transfer_hint = Some(TransferHint::Payment);
        let mut v12 = TransactionsVault {
            transactions: vec![
                tx_both,
                make_tx(
                    "in_ew",
                    "com.example.ew",
                    "e1",
                    MoneySourceType::Ewallet,
                    TxDirection::In,
                    50_000_000,
                    "IDR",
                    base_ts + 2 * min_ms,
                ),
            ],
            liability_events: vec![make_bill_paid(
                "ev1",
                "com.example.shop",
                "cc1",
                MoneySourceType::CreditCard,
                None,
                base_ts + min_ms,
            )],
            ..Default::default()
        };
        recompute_matches(&mut v12, 15);
        assert_eq!(
            v12.transactions[0].classification,
            TxClassification::Transfer
        );
        assert_eq!(
            v12.transactions[1].classification,
            TxClassification::Transfer
        );
        assert_eq!(
            v12.liability_events[0].status,
            LiabilityEventStatus::UnmatchedNeedsReview
        );

        // Extra A: Bill-paid collapse (two events on same source+sub_account within +-W -> earliest kept, second duplicate & hidden)
        let mut v_col = TransactionsVault {
            liability_events: vec![
                make_bill_paid(
                    "ev_late",
                    "com.example.wallet",
                    "pl1",
                    MoneySourceType::Paylater,
                    None,
                    base_ts + 2 * min_ms,
                ),
                make_bill_paid(
                    "ev_early",
                    "com.example.wallet",
                    "pl1",
                    MoneySourceType::Paylater,
                    None,
                    base_ts,
                ),
            ],
            ..Default::default()
        };
        recompute_matches(&mut v_col, 15);
        assert_eq!(
            v_col.liability_events[0].status,
            LiabilityEventStatus::Ignored
        );
        assert_eq!(
            v_col.liability_events[0].duplicate_of.as_deref(),
            Some("ev_early")
        );
        assert_eq!(
            v_col.liability_events[1].status,
            LiabilityEventStatus::UnmatchedNeedsReview
        );
        assert_eq!(v_col.liability_events[1].duplicate_of, None);

        // Extra B: Unpaired repayment OUT stays DebtPayment
        let mut tx_rep = make_tx(
            "rep_out",
            "com.example.wallet",
            "ew1",
            MoneySourceType::Ewallet,
            TxDirection::Out,
            15_000_000,
            "IDR",
            base_ts,
        );
        tx_rep.transfer_hint = Some(TransferHint::Repayment);
        let mut v_rep = TransactionsVault {
            transactions: vec![tx_rep],
            ..Default::default()
        };
        recompute_matches(&mut v_rep, 15);
        assert_eq!(
            v_rep.transactions[0].classification,
            TxClassification::DebtPayment
        );

        // Extra C: Shape 7 + 8 + 9 sequence -> 1 DebtPayment OUT + 1 Linked event (9 collapsed)
        let mut tx7 = make_tx(
            "s7",
            "com.example.wallet",
            "ew1",
            MoneySourceType::Ewallet,
            TxDirection::Out,
            15_000_000,
            "IDR",
            base_ts,
        );
        tx7.transfer_hint = Some(TransferHint::Repayment);
        let mut v_789 = TransactionsVault {
            transactions: vec![tx7],
            liability_events: vec![
                make_bill_paid(
                    "s8",
                    "com.example.wallet",
                    "pl1",
                    MoneySourceType::Paylater,
                    None,
                    base_ts + 5_000,
                ),
                make_bill_paid(
                    "s9",
                    "com.example.wallet",
                    "pl1",
                    MoneySourceType::Paylater,
                    None,
                    base_ts + 10_000,
                ),
            ],
            ..Default::default()
        };
        recompute_matches(&mut v_789, 15);
        assert_eq!(
            v_789.transactions[0].classification,
            TxClassification::DebtPayment
        );
        assert_eq!(
            v_789.transactions[0].linked_liability_event_id.as_deref(),
            Some("s8")
        );
        assert_eq!(
            v_789.liability_events[0].status,
            LiabilityEventStatus::Linked
        );
        assert_eq!(
            v_789.liability_events[0].linked_tx_id.as_deref(),
            Some("s7")
        );
        assert_eq!(
            v_789.liability_events[1].status,
            LiabilityEventStatus::Ignored
        );
        assert_eq!(
            v_789.liability_events[1].duplicate_of.as_deref(),
            Some("s8")
        );
    }

    #[cfg(windows)]
    #[test]
    fn end_to_end_dumpsys_ingest_idempotent_vault_and_privacy() {
        let dumpsys_raw = r#"
NotificationRecord(0x1111: pkg=com.example.bank user=UserHandle{0} id=1 tag=null key=0|com.example.bank|1|null|10001: Notification(channel=tx))
  postTime=1700000100000
  extras={
    android.title=String (Financial Diary)
    android.text=String (You received IDR 5,000,000.00 from Secret Employer at Salary.)
  }
NotificationRecord(0x2222: pkg=com.example.shop user=UserHandle{0} id=2 tag=null key=0|com.example.shop|2|null|10002: Notification(channel=cc))
  postTime=1700000200000
  extras={
    android.title=String (Transaksi Pembayaran Tagihan Kartu Kredit Anda Berhasil)
    android.text=String (Pembayaran tagihan kartu kredit Sample Card untuk Alex: 4111-22**-****-0042 Anda BERHASIL pada tanggal 25 Nov)
  }
NotificationRecord(0x3333: pkg=com.example.bank user=UserHandle{0} id=3 tag=null key=0|com.example.bank|3|null|10003: Notification(channel=tx))
  postTime=1700000150000
  extras={
    android.title=String (Payment)
    android.text=String (Payment OUT Rp 250.000,00 — Card Bill)
  }
"#;

        let parsed = parse_dumpsys_notifications_rich(dumpsys_raw);
        let items: Vec<PhoneHubNotification> = parsed
            .into_iter()
            .map(|r| PhoneHubNotification {
                key: r.base.key,
                package: r.base.package,
                app_label: r.base.app_label.unwrap_or_else(|| "Example".to_string()),
                title: r.base.title,
                text: r.base.text,
                posted_unix: r.base.posted_unix,
            })
            .collect();
        assert_eq!(items.len(), 3);

        let dir = temp_dir("e2e");
        let mut store = VaultStore::new_with_dir(dir.clone());
        let mut per_app = HashMap::new();
        per_app.insert(
            "com.example.bank".to_string(),
            policy_with_sources(
                RetentionPolicy::Transactions,
                vec![src("b1", MoneySourceType::Bank, "generic_bank")],
            ),
        );
        per_app.insert(
            "com.example.shop".to_string(),
            policy_with_sources(
                RetentionPolicy::Transactions,
                vec![src(
                    "cc1",
                    MoneySourceType::CreditCard,
                    "generic_credit_card",
                )],
            ),
        );
        let cfg = NotificationStorageConfig {
            history_enabled: true,
            per_app,
            ..Default::default()
        };
        store.apply_config(cfg.clone(), 1_700_000_500).unwrap();

        let wrote_first = store
            .ingest_notifications("HW1", &items, 1_700_000_500)
            .unwrap();
        assert!(wrote_first);
        assert_eq!(store.transactions_vault().transactions.len(), 2);
        assert_eq!(store.transactions_vault().liability_events.len(), 1);
        assert!(!dir.join(HISTORY_VAULT_FILENAME).exists());
        assert!(dir.join(TRANSACTIONS_VAULT_FILENAME).exists());

        // Second identical ingest must add 0 records and perform 0 writes
        let wrote_second = store
            .ingest_notifications("HW1", &items, 1_700_000_510)
            .unwrap();
        assert!(!wrote_second);
        assert_eq!(store.transactions_vault().transactions.len(), 2);
        assert_eq!(store.transactions_vault().liability_events.len(), 1);

        // Reload from encrypted DPAPI vault and check snapshot + zero leaks
        let mut reloaded = VaultStore::new_with_dir(dir.clone());
        reloaded.apply_config(cfg, 1_700_000_520).unwrap();
        let snap = build_money_snapshot(&mut reloaded, Some("HW1"), None, None, None).unwrap();
        assert_eq!(snap.transactions.len(), 2);
        assert_eq!(snap.liability_events.len(), 1);
        assert_eq!(
            snap.liability_events[0].status,
            LiabilityEventStatus::Linked
        );
        assert_eq!(
            snap.liability_events[0].resolved_amount_minor,
            Some(25_000_000)
        );

        let serialized = serde_json::to_string(reloaded.transactions_vault()).unwrap();
        assert!(!serialized.contains("0042"));
        assert!(!serialized.contains("4111"));
        assert!(!serialized.contains("Secret Employer"));
        assert!(!serialized.contains("Card Bill"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn fictional_encrypted_dumpsys_exercises_harness_pipeline() {
        let dumpsys_1 = r#"
NotificationRecord(0x1: pkg=com.example.wallet user=UserHandle{0} id=1 tag=null key=0|com.example.wallet|1|null|10001: Notification(channel=ew))
  postTime=1700001000000
  extras={
    android.title=String (Uang terkirim!)
    android.text=String (Kamu berhasil bayar sebesar Rp 150.000 buat InstallmentService)
  }
NotificationRecord(0x2: pkg=com.example.wallet user=UserHandle{0} id=2 tag=null key=0|com.example.wallet|2|null|10002: Notification(channel=pl))
  postTime=1700001005000
  extras={
    android.title=String (Yey! Pembayaran InstallmentService berhasil)
    android.text=String (Limit InstallmentService kamu juga udah keisi lagi, ya.)
  }
"#;

        let dumpsys_2 = r#"
NotificationRecord(0x2: pkg=com.example.wallet user=UserHandle{0} id=2 tag=null key=0|com.example.wallet|2|null|10002: Notification(channel=pl))
  postTime=1700001005000
  extras={
    android.title=String (Yey! Pembayaran InstallmentService berhasil)
    android.text=String (Limit InstallmentService kamu juga udah keisi lagi, ya.)
  }
NotificationRecord(0x3: pkg=com.example.wallet user=UserHandle{0} id=3 tag=null key=0|com.example.wallet|3|null|10003: Notification(channel=pl))
  postTime=1700001010000
  extras={
    android.title=String (Pembayaran tagihan berhasil)
    android.text=String (Limit InstallmentService kamu juga udah keisi lagi, ya.)
  }
NotificationRecord(0x4: pkg=com.example.wallet user=UserHandle{0} id=4 tag=null key=0|com.example.wallet|4|null|10004: Notification(channel=promo))
  postTime=1700001020000
  extras={
    android.title=String (Promo)
    android.text=String (Voucher diskon Rp 20.000 hari ini saja)
  }
"#;

        let enc1 = dpapi_protect_with_entropy(dumpsys_1.as_bytes(), &[]).unwrap();
        let enc2 = dpapi_protect_with_entropy(dumpsys_2.as_bytes(), &[]).unwrap();
        let dec1 = String::from_utf8_lossy(&dpapi_unprotect_with_entropy(&enc1, &[]).unwrap())
            .into_owned();
        let dec2 = String::from_utf8_lossy(&dpapi_unprotect_with_entropy(&enc2, &[]).unwrap())
            .into_owned();

        let sources_json = r#"{
            "version": 2,
            "historyEnabled": true,
            "backgroundCapture": false,
            "transactionRetention": "1y",
            "transferMatchWindowMinutes": 15,
            "largeIncomeThresholdByCurrency": {},
            "perApp": {
                "com.example.wallet": {
                    "retention": "transactions",
                    "retentionDays": 30,
                    "sensitive": true,
                    "moneySources": [
                        { "id": "ew1", "type": "ewallet", "ruleTemplate": "generic_ewallet", "enabled": true },
                        { "id": "pl1", "type": "paylater", "ruleTemplate": "generic_paylater", "enabled": true }
                    ]
                }
            }
        }"#;
        let cfg = parse_harness_sources_json(sources_json).unwrap();
        let report = run_capture_harness_from_dumpsys(&[&dec1, &dec2], &cfg, None);

        assert_eq!(report.total_records_deduped, 4);
        assert_eq!(report.by_source_type["ewallet"].parsed_out, 1);
        assert_eq!(report.by_source_type["ewallet"].debt_payments, 1);
        assert_eq!(report.by_source_type["paylater"].bill_paid_emitted, 2);
        assert_eq!(report.by_source_type["paylater"].bill_paid_collapsed, 1);
        assert_eq!(report.by_source_type["paylater"].bill_paid_linked, 1);
        assert_eq!(report.by_source_type["ewallet"].amount_lines_unmatched, 1);
    }

    #[cfg(windows)]
    #[test]
    fn review_actions_update_classifications_and_persist_to_vault() {
        let dir = temp_dir("review_actions");
        let mut store = VaultStore::new_with_dir(dir.clone());
        let mut per_app = HashMap::new();
        per_app.insert(
            "com.example.bank".to_string(),
            policy_with_sources(
                RetentionPolicy::Transactions,
                vec![src("b1", MoneySourceType::Bank, "generic_bank")],
            ),
        );
        per_app.insert(
            "com.example.shop".to_string(),
            policy_with_sources(
                RetentionPolicy::Transactions,
                vec![src(
                    "cc1",
                    MoneySourceType::CreditCard,
                    "generic_credit_card",
                )],
            ),
        );
        let cfg = NotificationStorageConfig {
            history_enabled: true,
            per_app,
            ..Default::default()
        };
        store.apply_config(cfg.clone(), 1_700_000_000).unwrap();

        let n_va = make_notif(
            "com.example.bank",
            "k_va",
            "Transfer",
            "Transfer ke Virtual Account Rp 200.000",
            1_700_000_010,
        );
        let n_bill = make_notif(
            "com.example.shop",
            "k_bill",
            "Tagihan",
            "Credit card bill payment for card 4111-22**-****-0042 succeeded on 25 Nov",
            1_700_005_000,
        );
        store
            .ingest_notifications("HW1", &[n_va, n_bill], 1_700_005_010)
            .unwrap();

        let snap0 = build_money_snapshot(&mut store, Some("HW1"), None, None, None).unwrap();
        assert_eq!(snap0.transactions.len(), 1);
        assert_eq!(
            snap0.transactions[0].classification,
            TxClassification::PossibleTransfer
        );
        assert_eq!(snap0.liability_events.len(), 1);
        assert_eq!(
            snap0.liability_events[0].status,
            LiabilityEventStatus::UnmatchedNeedsReview
        );

        let tx_id = snap0.transactions[0].id.clone();
        let ev_id = snap0.liability_events[0].id.clone();

        let res_tx = apply_transaction_correction_in_vault(
            &mut store,
            &tx_id,
            Some(TxClassification::Normal),
            None,
            false,
        )
        .unwrap();
        assert_eq!(res_tx.transaction.classification, TxClassification::Normal);
        assert_eq!(
            res_tx.transaction.manual_classification,
            Some(TxClassification::Normal)
        );

        let res_ev = resolve_liability_event_in_vault(&mut store, &ev_id, None, true).unwrap();
        assert_eq!(res_ev.event.status, LiabilityEventStatus::Ignored);
        assert!(res_ev.event.manual_locked);

        let mut reloaded = VaultStore::new_with_dir(dir.clone());
        reloaded.apply_config(cfg, 1_700_005_020).unwrap();
        let snap1 = build_money_snapshot(&mut reloaded, Some("HW1"), None, None, None).unwrap();
        assert_eq!(
            snap1.transactions[0].classification,
            TxClassification::Normal
        );
        assert_eq!(
            snap1.liability_events[0].status,
            LiabilityEventStatus::Ignored
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[ignore]
    #[test]
    fn real_capture_harness() {
        let Ok(capture_paths_raw) = std::env::var("ANYLEAP_REAL_CAPTURE_PATH") else {
            println!("skipped");
            return;
        };
        let Ok(sources_path) = std::env::var("ANYLEAP_REAL_SOURCES_PATH") else {
            println!("skipped");
            return;
        };
        if capture_paths_raw.trim().is_empty() || sources_path.trim().is_empty() {
            println!("skipped");
            return;
        }

        let sources_raw = fs::read_to_string(&sources_path).expect("failed to read sources config");
        let sources_cfg = parse_harness_sources_json(&sources_raw).expect("invalid sources config");

        let user_rules = std::env::var("ANYLEAP_REAL_RULES_PATH")
            .ok()
            .filter(|p| !p.trim().is_empty())
            .map(|p| fs::read_to_string(p).expect("failed to read user rules"));

        let mut decrypted_captures: Vec<String> = Vec::new();
        for path in capture_paths_raw
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let cipher = fs::read(path).expect("failed to read encrypted capture file");
            let plain = dpapi_unprotect_with_entropy(&cipher, &[])
                .expect("failed to decrypt DPAPI capture file");
            decrypted_captures.push(String::from_utf8_lossy(&plain).into_owned());
        }

        let refs: Vec<&str> = decrypted_captures.iter().map(String::as_str).collect();
        let report = run_capture_harness_from_dumpsys(&refs, &sources_cfg, user_rules.as_deref());
        print_harness_report(&report);
    }
}
