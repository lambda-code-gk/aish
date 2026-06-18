//! Smart Preprocessor / Local Intent Router — 純関数・DTO・feature hashing。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub const FEATURE_EXTRACTOR_VERSION: &str = "smart-features-v1";
pub const DEFAULT_MODEL_VERSION: &str = "smart-lr-v1";

/// basis points: 0..=10000 (= 0.0..=1.0)
pub type BasisPoints = u16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmartPreprocessMode {
    Off,
    Shadow,
    Assist,
    Gate,
}

impl SmartPreprocessMode {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "off" => Some(Self::Off),
            "shadow" => Some(Self::Shadow),
            "assist" => Some(Self::Assist),
            "gate" => Some(Self::Gate),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmartConfidenceGate {
    ForceRouteTurn,
    AssistRouteTurn,
    ShortCircuitAllowed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmartIntentClass {
    SimpleChat,
    Inspect,
    Debug,
    MemoryLookup,
    MemoryRecipeHint,
    ShellCommandCandidate,
    Retry,
    Rerun,
    Ambiguous,
    Unknown,
}

impl SmartIntentClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SimpleChat => "simple_chat",
            Self::Inspect => "inspect",
            Self::Debug => "debug",
            Self::MemoryLookup => "memory_lookup",
            Self::MemoryRecipeHint => "memory_recipe_hint",
            Self::ShellCommandCandidate => "shell_command_candidate",
            Self::Retry => "retry",
            Self::Rerun => "rerun",
            Self::Ambiguous => "ambiguous",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse_shortcut(raw: &str) -> Option<Self> {
        match raw.trim() {
            "simple_chat" => Some(Self::SimpleChat),
            "retry" => Some(Self::Retry),
            "rerun" => Some(Self::Rerun),
            "memory_lookup" => Some(Self::MemoryLookup),
            "inspect" => Some(Self::Inspect),
            "debug" => Some(Self::Debug),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmartRouteTurnHints {
    pub recent_summary: Option<String>,
    pub new_conversation: bool,
    pub conversation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmartSafetySummary {
    pub requires_approval: bool,
    pub contains_secret_risk: bool,
    pub contains_write_risk: bool,
    pub contains_network_risk: bool,
}

impl SmartSafetySummary {
    pub fn is_safe_for_short_circuit(&self) -> bool {
        !self.requires_approval
            && !self.contains_secret_risk
            && !self.contains_write_risk
            && !self.contains_network_risk
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmartEvidence {
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SmartHeadScores {
    pub route_bps: BasisPoints,
    pub failure_kind_bps: BasisPoints,
    pub context_need_bps: BasisPoints,
    pub tool_hint_bps: BasisPoints,
    pub output_style_bps: BasisPoints,
    pub risk_hint_bps: BasisPoints,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SmartPreprocessDecision {
    pub version: u32,
    pub mode: SmartPreprocessMode,
    pub intent: SmartIntentClass,
    pub confidence_bps: BasisPoints,
    pub gate: SmartConfidenceGate,
    pub route_turn_required: bool,
    pub route_turn_hints: SmartRouteTurnHints,
    pub safety: SmartSafetySummary,
    pub evidence: Vec<SmartEvidence>,
    pub head_scores: SmartHeadScores,
    pub model_version: Option<String>,
    pub feature_hash_version: String,
    pub reason_codes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PreprocessInput {
    pub user_text: String,
    pub command: Option<String>,
    pub tty: bool,
    pub new_conversation: bool,
    pub conversation_id: Option<String>,
    pub memory_enabled: bool,
    pub history_tail_summary: Option<String>,
    pub session_error_summary: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PreprocessConfig {
    pub mode: SmartPreprocessMode,
    pub route_turn_threshold_bps: BasisPoints,
    pub assist_threshold_bps: BasisPoints,
    pub max_evidence_bytes: usize,
    pub feature_hash_buckets: u32,
    pub feature_hash_seed: u64,
    pub allow_shortcuts: Vec<SmartIntentClass>,
    pub model_version: Option<String>,
    /// `gate` 短絡は検証済み model artifact がある場合のみ許可する。
    pub gate_model_verified: bool,
}

impl Default for PreprocessConfig {
    fn default() -> Self {
        Self {
            mode: SmartPreprocessMode::Shadow,
            route_turn_threshold_bps: 8500,
            assist_threshold_bps: 9500,
            max_evidence_bytes: 4096,
            feature_hash_buckets: 262144,
            feature_hash_seed: 17,
            allow_shortcuts: vec![SmartIntentClass::SimpleChat],
            model_version: Some(DEFAULT_MODEL_VERSION.to_string()),
            gate_model_verified: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashedFeature {
    pub index: u32,
    pub value: i8,
}

pub fn hash_feature(name: &str, buckets: u32, seed: u64) -> u32 {
    let mut hash = seed;
    for byte in name.as_bytes() {
        hash = hash
            .wrapping_mul(0x100000001b3)
            .wrapping_add(u64::from(*byte));
    }
    (hash % u64::from(buckets.max(1))) as u32
}

pub fn extract_ngram_features(source: &str, text: &str, n: usize) -> Vec<String> {
    let normalized = normalize_text(text);
    let chars: Vec<char> = normalized.chars().collect();
    if chars.len() < n {
        return Vec::new();
    }
    let mut out = Vec::new();
    for window in chars.windows(n) {
        let gram: String = window.iter().collect();
        out.push(format!("{source}_ngram:{gram}"));
    }
    out
}

pub fn normalize_text(input: &str) -> String {
    input.to_ascii_lowercase()
}

pub fn redact_for_evidence(input: &str, max_len: usize) -> String {
    let mut out = String::new();
    for word in input.split_whitespace() {
        if word.starts_with('/') || word.starts_with("~/") || word.contains('\\') {
            out.push_str("[path] ");
            continue;
        }
        if looks_like_secret(word) {
            out.push_str("[redacted] ");
            continue;
        }
        out.push_str(word);
        out.push(' ');
    }
    let trimmed = out.trim();
    truncate_with_ellipsis(trimmed, max_len)
}

pub fn looks_like_secret(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    lower.contains("api_key")
        || lower.contains("apikey")
        || lower.contains("secret")
        || lower.contains("password")
        || lower.contains("token")
        || lower.starts_with("sk-")
        || lower.starts_with("ghp_")
        || lower.starts_with("gho_")
}

pub fn extract_boolean_signals(input: &PreprocessInput) -> HashMap<String, bool> {
    let text = normalize_text(&input.user_text);
    let mut signals = HashMap::new();
    signals.insert(
        "user:asks_fix".into(),
        contains_any(&text, &["直して", "修正", "fix", "debug"]),
    );
    signals.insert(
        "user:asks_explain".into(),
        contains_any(&text, &["説明", "explain", "what does", "何をする"]),
    );
    signals.insert(
        "user:asks_git".into(),
        contains_any(&text, &["git", "コミット", "差分", "commit"]),
    );
    signals.insert(
        "user:asks_memory".into(),
        contains_any(&text, &["前に", "覚え", "memory", "設計方針"]),
    );
    signals.insert(
        "user:asks_short_output".into(),
        contains_any(&text, &["短く", "コマンドだけ", "concise"]),
    );
    signals.insert(
        "user:mentions_previous".into(),
        contains_any(&text, &["さっき", "前の", "直前"]),
    );
    signals.insert(
        "command:retry".into(),
        input.command.as_deref() == Some("retry"),
    );
    signals.insert(
        "command:rerun".into(),
        input.command.as_deref() == Some("rerun"),
    );
    signals.insert(
        "session:has_error".into(),
        input.session_error_summary.is_some(),
    );
    signals.insert(
        "session:task_exists".into(),
        input.history_tail_summary.is_some(),
    );
    signals.insert("git:memory_disabled".into(), !input.memory_enabled);
    signals
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| text.contains(n))
}

fn truncate_to_char_boundary(input: &str, max_len: usize) -> &str {
    if max_len >= input.len() {
        return input;
    }
    let mut end = max_len;
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    &input[..end]
}

fn truncate_with_ellipsis(input: &str, max_len: usize) -> String {
    if input.len() <= max_len {
        return input.to_string();
    }
    if max_len == 0 {
        return String::new();
    }
    if max_len <= 3 {
        return truncate_to_char_boundary(input, max_len).to_string();
    }
    let cutoff = truncate_to_char_boundary(input, max_len - 3);
    format!("{cutoff}...")
}

pub fn build_hashed_features(
    input: &PreprocessInput,
    buckets: u32,
    seed: u64,
) -> Vec<HashedFeature> {
    let mut names = Vec::new();
    names.extend(extract_ngram_features("user", &input.user_text, 2));
    names.extend(extract_ngram_features("user", &input.user_text, 3));
    if let Some(ref summary) = input.session_error_summary {
        names.extend(extract_ngram_features("stderr", summary, 2));
    }
    if let Some(ref summary) = input.history_tail_summary {
        names.extend(extract_ngram_features("history", summary, 2));
    }
    for (key, enabled) in extract_boolean_signals(input) {
        if enabled {
            names.push(key);
        }
    }
    let mut seen = HashMap::<u32, i8>::new();
    for name in names {
        let idx = hash_feature(&name, buckets, seed);
        let entry = seen.entry(idx).or_insert(0);
        *entry = entry.saturating_add(1).min(3);
    }
    seen.into_iter()
        .map(|(index, value)| HashedFeature { index, value })
        .collect()
}

pub fn classify_local(
    input: &PreprocessInput,
    features: &[HashedFeature],
) -> (SmartIntentClass, BasisPoints, Vec<String>) {
    let text = normalize_text(&input.user_text);
    let mut reason_codes = Vec::new();
    let mut scores: HashMap<SmartIntentClass, i32> = HashMap::new();

    if input.command.as_deref() == Some("retry") {
        *scores.entry(SmartIntentClass::Retry).or_default() += 100;
        reason_codes.push("command_retry".into());
    }
    if input.command.as_deref() == Some("rerun") {
        *scores.entry(SmartIntentClass::Rerun).or_default() += 100;
        reason_codes.push("command_rerun".into());
    }
    if contains_any(&text, &["直して", "修正", "fix", "エラー"]) {
        *scores.entry(SmartIntentClass::Debug).or_default() += 60;
        reason_codes.push("user_asks_fix".into());
    }
    if contains_any(&text, &["git", "コミット", "差分"]) {
        *scores.entry(SmartIntentClass::Inspect).or_default() += 55;
        reason_codes.push("user_asks_git".into());
    }
    if contains_any(&text, &["前に", "覚え", "memory", "設計方針"]) && input.memory_enabled
    {
        *scores.entry(SmartIntentClass::MemoryLookup).or_default() += 65;
        reason_codes.push("user_asks_memory".into());
    }
    if contains_any(&text, &["説明", "何をする", "explain"]) {
        *scores.entry(SmartIntentClass::Inspect).or_default() += 45;
        reason_codes.push("user_asks_explain".into());
    }
    if contains_any(&text, &["sudo", "rm ", "chmod", "curl", "wget"]) {
        *scores
            .entry(SmartIntentClass::ShellCommandCandidate)
            .or_default() += 80;
        reason_codes.push("shell_command_candidate".into());
    }
    if text.len() < 40 && !scores.contains_key(&SmartIntentClass::ShellCommandCandidate) {
        *scores.entry(SmartIntentClass::SimpleChat).or_default() += 30;
    }
    if !features.is_empty() && scores.is_empty() {
        *scores.entry(SmartIntentClass::Ambiguous).or_default() += 20;
    }
    if scores.is_empty() {
        return (SmartIntentClass::Unknown, 3000, reason_codes);
    }
    let mut ranked: Vec<_> = scores.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.as_str().cmp(b.0.as_str())));
    let (intent, top) = ranked[0];
    let second = ranked.get(1).map(|(_, s)| *s).unwrap_or(0);
    let margin = (top - second).max(0) as u16;
    let base = (top * 80).min(9800) as u16;
    let confidence = base.saturating_add(margin.saturating_mul(10)).min(10000);
    (intent, confidence, reason_codes)
}

pub fn assess_safety(input: &PreprocessInput) -> SmartSafetySummary {
    let text = normalize_text(&input.user_text);
    SmartSafetySummary {
        requires_approval: contains_any(&text, &["sudo", "rm ", "chmod", "chown", "dd ", "mkfs"]),
        contains_secret_risk: input.user_text.split_whitespace().any(looks_like_secret),
        contains_write_risk: contains_any(&text, &["書き", "delete", "remove", "truncate", "> "]),
        contains_network_risk: contains_any(&text, &["curl", "wget", "http://", "https://"]),
    }
}

pub fn apply_confidence_gate(
    intent: SmartIntentClass,
    confidence_bps: BasisPoints,
    safety: &SmartSafetySummary,
    config: &PreprocessConfig,
) -> SmartConfidenceGate {
    if !safety.is_safe_for_short_circuit() {
        return SmartConfidenceGate::ForceRouteTurn;
    }
    match intent {
        SmartIntentClass::Ambiguous
        | SmartIntentClass::Unknown
        | SmartIntentClass::ShellCommandCandidate => SmartConfidenceGate::ForceRouteTurn,
        _ if config.mode == SmartPreprocessMode::Gate
            && confidence_bps >= config.route_turn_threshold_bps =>
        {
            SmartConfidenceGate::ShortCircuitAllowed
        }
        _ if confidence_bps >= config.route_turn_threshold_bps => {
            SmartConfidenceGate::AssistRouteTurn
        }
        _ => SmartConfidenceGate::ForceRouteTurn,
    }
}

pub fn build_bounded_summary(input: &PreprocessInput, max_bytes: usize) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(ref h) = input.history_tail_summary {
        parts.push(format!("history: {}", redact_for_evidence(h, 120)));
    }
    if let Some(ref e) = input.session_error_summary {
        parts.push(format!("session_error: {}", redact_for_evidence(e, 120)));
    }
    if parts.is_empty() {
        return None;
    }
    let joined = parts.join("; ");
    Some(redact_for_evidence(&joined, max_bytes))
}

pub fn run_preprocessor(
    input: PreprocessInput,
    config: &PreprocessConfig,
) -> SmartPreprocessDecision {
    let features = build_hashed_features(
        &input,
        config.feature_hash_buckets,
        config.feature_hash_seed,
    );
    let (intent, confidence_bps, mut reason_codes) = classify_local(&input, &features);
    let safety = assess_safety(&input);
    let gate = apply_confidence_gate(intent, confidence_bps, &safety, config);
    let route_turn_required = match config.mode {
        SmartPreprocessMode::Off => true,
        SmartPreprocessMode::Shadow => true,
        SmartPreprocessMode::Assist => true,
        SmartPreprocessMode::Gate => {
            !(gate == SmartConfidenceGate::ShortCircuitAllowed
                && config.allow_shortcuts.contains(&intent)
                && safety.is_safe_for_short_circuit()
                && config.gate_model_verified)
        }
    };
    if !route_turn_required {
        reason_codes.push("gate_short_circuit".into());
    }
    let recent_summary = match config.mode {
        SmartPreprocessMode::Assist => build_bounded_summary(&input, config.max_evidence_bytes),
        SmartPreprocessMode::Gate if gate != SmartConfidenceGate::ForceRouteTurn => {
            build_bounded_summary(&input, config.max_evidence_bytes)
        }
        _ => None,
    };
    let mut evidence = Vec::new();
    evidence.push(SmartEvidence {
        kind: "user_excerpt".into(),
        value: redact_for_evidence(&input.user_text, 200),
    });
    if let Some(ref summary) = recent_summary {
        evidence.push(SmartEvidence {
            kind: "bounded_summary".into(),
            value: summary.clone(),
        });
    }
    SmartPreprocessDecision {
        version: 1,
        mode: config.mode,
        intent,
        confidence_bps,
        gate,
        route_turn_required,
        route_turn_hints: SmartRouteTurnHints {
            recent_summary,
            new_conversation: input.new_conversation,
            conversation_id: input.conversation_id.clone(),
        },
        safety,
        evidence,
        head_scores: SmartHeadScores {
            route_bps: confidence_bps,
            ..SmartHeadScores::default()
        },
        model_version: config.model_version.clone(),
        feature_hash_version: FEATURE_EXTRACTOR_VERSION.to_string(),
        reason_codes,
    }
}

pub fn should_short_circuit(decision: &SmartPreprocessDecision) -> bool {
    decision.mode == SmartPreprocessMode::Gate
        && !decision.route_turn_required
        && decision.gate == SmartConfidenceGate::ShortCircuitAllowed
        && decision.safety.is_safe_for_short_circuit()
        && decision.model_version.as_deref() == Some(DEFAULT_MODEL_VERSION)
        && decision.feature_hash_version == FEATURE_EXTRACTOR_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_input(text: &str) -> PreprocessInput {
        PreprocessInput {
            user_text: text.into(),
            command: Some("ask".into()),
            tty: true,
            new_conversation: true,
            conversation_id: None,
            memory_enabled: true,
            history_tail_summary: None,
            session_error_summary: None,
        }
    }

    #[test]
    fn hash_index_is_stable() {
        let a = hash_feature("user_ngram:エラー", 4096, 17);
        let b = hash_feature("user_ngram:エラー", 4096, 17);
        assert_eq!(a, b);
        let c = hash_feature("user_ngram:エラー", 4096, 18);
        assert_ne!(a, c);
    }

    #[test]
    fn fix_error_route_golden() {
        let mut input = sample_input("さっきのエラーを直して");
        input
            .session_error_summary
            .get_or_insert("test failed".into());
        let decision = run_preprocessor(input, &PreprocessConfig::default());
        assert!(matches!(
            decision.intent,
            SmartIntentClass::Debug | SmartIntentClass::Ambiguous
        ));
        assert!(decision.route_turn_required);
    }

    #[test]
    fn git_assist_golden() {
        let input = sample_input("この差分をコミット単位に分けたい");
        let decision = run_preprocessor(input, &PreprocessConfig::default());
        assert_eq!(decision.intent, SmartIntentClass::Inspect);
    }

    #[test]
    fn memory_query_golden() {
        let input = sample_input("前に決めた設計方針を見たい");
        let decision = run_preprocessor(input, &PreprocessConfig::default());
        assert_eq!(decision.intent, SmartIntentClass::MemoryLookup);
    }

    #[test]
    fn confidence_gate_high_medium_low() {
        let safety = SmartSafetySummary {
            requires_approval: false,
            contains_secret_risk: false,
            contains_write_risk: false,
            contains_network_risk: false,
        };
        let mut gate_cfg = PreprocessConfig::default();
        gate_cfg.mode = SmartPreprocessMode::Gate;
        assert_eq!(
            apply_confidence_gate(SmartIntentClass::SimpleChat, 8600, &safety, &gate_cfg),
            SmartConfidenceGate::ShortCircuitAllowed
        );
        let cfg = PreprocessConfig::default();
        assert_eq!(
            apply_confidence_gate(SmartIntentClass::SimpleChat, 8600, &safety, &cfg),
            SmartConfidenceGate::AssistRouteTurn
        );
        assert_eq!(
            apply_confidence_gate(SmartIntentClass::SimpleChat, 4000, &safety, &cfg),
            SmartConfidenceGate::ForceRouteTurn
        );
    }

    #[test]
    fn dangerous_input_forces_route_turn() {
        let input = sample_input("sudo rm -rf /tmp/foo");
        let mut cfg = PreprocessConfig::default();
        cfg.mode = SmartPreprocessMode::Gate;
        let decision = run_preprocessor(input, &cfg);
        assert!(decision.route_turn_required);
        assert!(!decision.safety.is_safe_for_short_circuit());
    }

    #[test]
    fn serde_roundtrip_decision() {
        let input = sample_input("hello");
        let decision = run_preprocessor(input, &PreprocessConfig::default());
        let json = serde_json::to_string(&decision).expect("serialize");
        let back: SmartPreprocessDecision = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decision, back);
    }

    #[test]
    fn redaction_masks_secrets_and_paths() {
        let out = redact_for_evidence("token=ghp_abc /home/user/secret", 200);
        assert!(out.contains("[redacted]"));
        assert!(out.contains("[path]"));
    }

    #[test]
    fn redaction_truncates_unicode_without_panicking() {
        let out = redact_for_evidence("エラーを直してすぐ確認して", 11);
        assert!(!out.is_empty());
        assert!(out.ends_with("..."));
    }

    #[test]
    fn gate_without_verified_model_never_short_circuits() {
        let input = PreprocessInput {
            user_text: "retry".into(),
            command: Some("retry".into()),
            tty: true,
            new_conversation: true,
            conversation_id: None,
            memory_enabled: true,
            history_tail_summary: None,
            session_error_summary: None,
        };
        let mut cfg = PreprocessConfig::default();
        cfg.mode = SmartPreprocessMode::Gate;
        cfg.gate_model_verified = false;
        cfg.model_version = None;
        let decision = run_preprocessor(input, &cfg);
        assert!(decision.route_turn_required);
    }
}
