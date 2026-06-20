//! Smart Preprocessor / Local Intent Router — 純関数・DTO・feature hashing。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub const FEATURE_EXTRACTOR_VERSION: &str = "smart-features-v1";
pub const DEFAULT_MODEL_VERSION: &str = "smart-lr-v1";
pub const MAX_REASON_CODES: usize = 16;
pub const MAX_REASON_CODE_LEN: usize = 48;
pub const DEFAULT_HINT_THRESHOLD_BPS: BasisPoints = 5500;
pub const DEFAULT_SHORT_CIRCUIT_THRESHOLD_BPS: BasisPoints = 8500;

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

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Shadow => "shadow",
            Self::Assist => "assist",
            Self::Gate => "gate",
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

impl SmartConfidenceGate {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ForceRouteTurn => "force_route_turn",
            Self::AssistRouteTurn => "assist_route_turn",
            Self::ShortCircuitAllowed => "short_circuit_allowed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmartFailureKind {
    Build,
    Test,
    Runtime,
    Permission,
    Network,
    Dependency,
    Git,
    FileNotFound,
    CommandNotFound,
    Timeout,
    UnknownFailure,
}

impl SmartFailureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Test => "test",
            Self::Runtime => "runtime",
            Self::Permission => "permission",
            Self::Network => "network",
            Self::Dependency => "dependency",
            Self::Git => "git",
            Self::FileNotFound => "file_not_found",
            Self::CommandNotFound => "command_not_found",
            Self::Timeout => "timeout",
            Self::UnknownFailure => "unknown_failure",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmartContextNeed {
    LastCommand,
    ExitStatus,
    StderrTail,
    StdoutTail,
    ErrorLines,
    #[serde(alias = "git_status")]
    VcsStatus,
    #[serde(alias = "git_diff")]
    VcsDiff,
    ConversationTail,
    MemoryCards,
    ProjectRules,
}

impl SmartContextNeed {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LastCommand => "last_command",
            Self::ExitStatus => "exit_status",
            Self::StderrTail => "stderr_tail",
            Self::StdoutTail => "stdout_tail",
            Self::ErrorLines => "error_lines",
            Self::VcsStatus => "vcs_status",
            Self::VcsDiff => "vcs_diff",
            Self::ConversationTail => "conversation_tail",
            Self::MemoryCards => "memory_cards",
            Self::ProjectRules => "project_rules",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmartToolHint {
    GitStatus,
    GitDiff,
    Grep,
    ReadFile,
    ListDir,
    ShellExecCandidate,
    MemorySearch,
    ConversationSearch,
}

impl SmartToolHint {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GitStatus => "git_status",
            Self::GitDiff => "git_diff",
            Self::Grep => "grep",
            Self::ReadFile => "read_file",
            Self::ListDir => "list_dir",
            Self::ShellExecCandidate => "shell_exec_candidate",
            Self::MemorySearch => "memory_search",
            Self::ConversationSearch => "conversation_search",
        }
    }
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

    pub fn all_scored() -> [Self; 9] {
        [
            Self::SimpleChat,
            Self::Inspect,
            Self::Debug,
            Self::MemoryLookup,
            Self::MemoryRecipeHint,
            Self::ShellCommandCandidate,
            Self::Retry,
            Self::Rerun,
            Self::Ambiguous,
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SmartRouteTurnHints {
    pub recent_summary: Option<String>,
    pub new_conversation: bool,
    pub conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_needs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_hints: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preprocessor_intent: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preprocessor_reason_codes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_bps: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_gate: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_requires_approval: Option<bool>,
}

impl SmartRouteTurnHints {
    pub fn has_preprocessor_wire_hints(&self) -> bool {
        !self.context_needs.is_empty()
            || !self.tool_hints.is_empty()
            || self.failure_kind.is_some()
            || self.preprocessor_intent.is_some()
            || !self.preprocessor_reason_codes.is_empty()
            || self.confidence_bps.is_some()
            || self.confidence_gate.is_some()
            || self.safety_requires_approval.is_some()
    }

    pub fn has_route_turn_hint_payload(&self) -> bool {
        self.recent_summary.is_some() || self.has_preprocessor_wire_hints()
    }
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
    pub intent_bps: BasisPoints,
    pub safety_bps: BasisPoints,
    pub gate_bps: BasisPoints,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SmartPreprocessDecision {
    pub version: u32,
    pub mode: SmartPreprocessMode,
    pub intent: SmartIntentClass,
    pub confidence_bps: BasisPoints,
    pub gate: SmartConfidenceGate,
    pub route_turn_required: bool,
    pub short_circuit_allowed: bool,
    pub inject_hints: bool,
    pub route_turn_hints: SmartRouteTurnHints,
    pub safety: SmartSafetySummary,
    pub evidence: Vec<SmartEvidence>,
    pub head_scores: SmartHeadScores,
    pub model_version: Option<String>,
    pub feature_hash_version: String,
    pub reason_codes: Vec<String>,
    pub failure_kind: Option<SmartFailureKind>,
    pub context_needs: Vec<SmartContextNeed>,
    pub tool_hints: Vec<SmartToolHint>,
    pub signal_feature_count: usize,
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
    pub cli_overrides: bool,
    pub route_metadata: RouteMetadataInput,
}

#[derive(Debug, Clone, Default)]
pub struct RouteMetadataInput {
    pub prior_route_kind: Option<String>,
    pub prior_route_fallback: bool,
    pub prior_required_approval: bool,
}

#[derive(Debug, Clone)]
pub struct SparseLogisticHead {
    pub bias: f32,
    pub weights: Vec<(u32, f32)>,
}

#[derive(Debug, Clone)]
pub struct PreprocessorModel {
    pub model_version: String,
    pub feature_extractor_version: String,
    pub intent_heads: HashMap<SmartIntentClass, SparseLogisticHead>,
    pub safety_head: SparseLogisticHead,
    pub gate_head: SparseLogisticHead,
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
    pub model: Option<PreprocessorModel>,
}

impl Default for PreprocessConfig {
    fn default() -> Self {
        Self {
            mode: SmartPreprocessMode::Shadow,
            route_turn_threshold_bps: DEFAULT_SHORT_CIRCUIT_THRESHOLD_BPS,
            assist_threshold_bps: DEFAULT_HINT_THRESHOLD_BPS,
            max_evidence_bytes: 4096,
            feature_hash_buckets: 262144,
            feature_hash_seed: 17,
            allow_shortcuts: vec![SmartIntentClass::SimpleChat],
            model: None,
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

pub const SAFETY_SHORT_CIRCUIT_MAX_BPS: BasisPoints = 3500;

pub fn redact_for_features(input: &str, max_len: usize) -> String {
    redact_for_evidence(input, max_len)
}

fn is_write_risk_text(text: &str) -> bool {
    contains_any(
        text,
        &[
            "書き", "delete", "remove", "truncate", "> ", "write", "store",
        ],
    )
}

fn is_memory_write_risk_text(text: &str) -> bool {
    contains_any(
        text,
        &[
            "メモして",
            "メモする",
            "覚えて",
            "保存して",
            "記録して",
            "remember",
            "save this",
            "store this",
        ],
    )
}

fn is_shell_risk_text(text: &str) -> bool {
    contains_any(
        text,
        &[
            "sudo", "rm ", "chmod", "chown", "dd ", "mkfs", "curl", "wget",
        ],
    )
}

fn is_simple_chat_candidate(text: &str) -> bool {
    text.len() < 40 && !is_shell_risk_text(text)
}

pub fn extract_boolean_signals(input: &PreprocessInput) -> HashMap<String, bool> {
    let text = normalize_text(&input.user_text);
    let asks_fix = contains_any(&text, &["直して", "修正", "fix", "debug", "エラー"]);
    let asks_git = contains_any(&text, &["git", "コミット", "差分", "commit"]);
    let asks_memory =
        contains_any(&text, &["前に", "覚え", "memory", "設計方針"]) && input.memory_enabled;
    let asks_explain = contains_any(&text, &["説明", "explain", "what does", "何をする"]);
    let memory_write = is_memory_write_risk_text(&text);
    let has_specific_intent = asks_fix || asks_git || asks_memory || asks_explain || memory_write;
    let mut signals = HashMap::new();
    signals.insert("user:asks_fix".into(), asks_fix);
    signals.insert("user:asks_explain".into(), asks_explain);
    signals.insert("user:asks_git".into(), asks_git);
    signals.insert("user:asks_memory".into(), asks_memory);
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
    signals.insert("context:non_tty".into(), !input.tty);
    signals.insert(
        "route:prior_fallback".into(),
        input.route_metadata.prior_route_fallback,
    );
    signals.insert(
        "route:prior_approval".into(),
        input.route_metadata.prior_required_approval,
    );
    if let Some(ref kind) = input.route_metadata.prior_route_kind {
        signals.insert(format!("route:kind:{kind}"), true);
    }
    signals.insert("signal:short_text".into(), text.len() < 40);
    signals.insert(
        "signal:simple_chat".into(),
        is_simple_chat_candidate(&text) && !has_specific_intent,
    );
    signals.insert("signal:shell_risk".into(), is_shell_risk_text(&text));
    signals.insert(
        "signal:secret_risk".into(),
        input.user_text.split_whitespace().any(looks_like_secret),
    );
    signals.insert(
        "signal:write_risk".into(),
        is_write_risk_text(&text) || memory_write,
    );
    signals.insert("signal:memory_write_risk".into(), memory_write);
    signals.insert(
        "signal:network_risk".into(),
        contains_any(&text, &["http://", "https://"]) || contains_any(&text, &["curl", "wget"]),
    );
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
    let user_for_features = redact_for_features(&input.user_text, 512);
    names.extend(extract_ngram_features("user", &user_for_features, 2));
    names.extend(extract_ngram_features("user", &user_for_features, 3));
    if let Some(ref summary) = input.session_error_summary {
        names.extend(extract_ngram_features("session_error", summary, 2));
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

fn sparse_dot(head: &SparseLogisticHead, features: &[HashedFeature]) -> f32 {
    let mut sum = head.bias;
    for hf in features {
        let x = f32::from(hf.value);
        for (idx, weight) in &head.weights {
            if *idx == hf.index {
                sum += weight * x;
            }
        }
    }
    sum
}

pub fn logit_to_bps(logit: f32) -> BasisPoints {
    if !logit.is_finite() {
        return 0;
    }
    let p = 1.0 / (1.0 + (-logit).exp());
    (p * 10_000.0).round().clamp(0.0, 10_000.0) as BasisPoints
}

pub fn classify_with_model(
    input: &PreprocessInput,
    features: &[HashedFeature],
    model: &PreprocessorModel,
) -> (SmartIntentClass, BasisPoints, SmartHeadScores, Vec<String>) {
    let mut reason_codes = Vec::new();
    let mut ranked: Vec<(SmartIntentClass, BasisPoints)> = Vec::new();
    for intent in SmartIntentClass::all_scored() {
        let head = model
            .intent_heads
            .get(&intent)
            .cloned()
            .unwrap_or(SparseLogisticHead {
                bias: -5.0,
                weights: Vec::new(),
            });
        ranked.push((intent, logit_to_bps(sparse_dot(&head, features))));
    }
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.as_str().cmp(b.0.as_str())));
    let (intent, intent_bps) = ranked[0];
    let second_bps = ranked.get(1).map(|(_, bps)| *bps).unwrap_or(0);
    if intent_bps.saturating_sub(second_bps) < 500 {
        reason_codes.push("intent_margin_low".into());
    }
    if matches!(intent, SmartIntentClass::SimpleChat) {
        reason_codes.push("model_simple_chat".into());
    }
    let safety_logit = sparse_dot(&model.safety_head, features);
    let safety_bps = logit_to_bps(safety_logit);
    let gate_bps = logit_to_bps(sparse_dot(&model.gate_head, features));
    let confidence_bps = gate_bps.min(intent_bps);
    if input.command.as_deref() == Some("retry") {
        reason_codes.push("command_retry".into());
        return (
            SmartIntentClass::Retry,
            confidence_bps,
            SmartHeadScores {
                intent_bps,
                safety_bps,
                gate_bps,
            },
            reason_codes,
        );
    }
    if input.command.as_deref() == Some("rerun") {
        reason_codes.push("command_rerun".into());
        return (
            SmartIntentClass::Rerun,
            confidence_bps,
            SmartHeadScores {
                intent_bps,
                safety_bps,
                gate_bps,
            },
            reason_codes,
        );
    }
    if !input.memory_enabled
        && matches!(
            intent,
            SmartIntentClass::MemoryLookup | SmartIntentClass::MemoryRecipeHint
        )
    {
        reason_codes.push("memory_disabled".into());
        return (
            SmartIntentClass::Inspect,
            confidence_bps,
            SmartHeadScores {
                intent_bps,
                safety_bps,
                gate_bps,
            },
            reason_codes,
        );
    }
    (
        intent,
        confidence_bps,
        SmartHeadScores {
            intent_bps,
            safety_bps,
            gate_bps,
        },
        reason_codes,
    )
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
    if is_simple_chat_candidate(&text)
        && !scores.contains_key(&SmartIntentClass::ShellCommandCandidate)
    {
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
    let memory_write = is_memory_write_risk_text(&text);
    SmartSafetySummary {
        requires_approval: contains_any(&text, &["sudo", "rm ", "chmod", "chown", "dd ", "mkfs"]),
        contains_secret_risk: input.user_text.split_whitespace().any(looks_like_secret),
        contains_write_risk: is_write_risk_text(&text) || memory_write,
        contains_network_risk: contains_any(&text, &["curl", "wget", "http://", "https://"]),
    }
}

pub fn count_signal_features(input: &PreprocessInput) -> usize {
    extract_boolean_signals(input)
        .values()
        .filter(|enabled| **enabled)
        .count()
}

pub fn apply_confidence_gate(
    intent: SmartIntentClass,
    confidence_bps: BasisPoints,
    safety: &SmartSafetySummary,
    head_scores: &SmartHeadScores,
    config: &PreprocessConfig,
) -> SmartConfidenceGate {
    if !safety.is_safe_for_short_circuit() {
        return SmartConfidenceGate::ForceRouteTurn;
    }
    if head_scores.safety_bps > SAFETY_SHORT_CIRCUIT_MAX_BPS {
        return SmartConfidenceGate::ForceRouteTurn;
    }
    let gate_confidence = confidence_bps.min(head_scores.gate_bps);
    match intent {
        SmartIntentClass::Ambiguous
        | SmartIntentClass::Unknown
        | SmartIntentClass::ShellCommandCandidate
        | SmartIntentClass::Retry
        | SmartIntentClass::Rerun
        | SmartIntentClass::MemoryLookup
        | SmartIntentClass::MemoryRecipeHint => SmartConfidenceGate::ForceRouteTurn,
        _ if config.mode == SmartPreprocessMode::Gate
            && gate_confidence >= config.route_turn_threshold_bps =>
        {
            SmartConfidenceGate::ShortCircuitAllowed
        }
        _ if gate_confidence >= config.assist_threshold_bps => SmartConfidenceGate::AssistRouteTurn,
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

pub fn clamp_reason_codes(mut codes: Vec<String>) -> Vec<String> {
    codes.truncate(MAX_REASON_CODES);
    codes
        .into_iter()
        .map(|code| {
            if code.len() <= MAX_REASON_CODE_LEN {
                return code;
            }
            truncate_with_ellipsis(&code, MAX_REASON_CODE_LEN)
        })
        .collect()
}

pub fn infer_failure_kind(session_error_summary: Option<&str>) -> Option<SmartFailureKind> {
    let text = session_error_summary?.to_ascii_lowercase();
    if text.contains("permission denied") {
        return Some(SmartFailureKind::Permission);
    }
    if text.contains("no such file or directory") {
        return Some(SmartFailureKind::FileNotFound);
    }
    if text.contains("command not found") {
        return Some(SmartFailureKind::CommandNotFound);
    }
    if text.contains("timed out") || text.contains("timeout") {
        return Some(SmartFailureKind::Timeout);
    }
    if text.contains("test failed") || text.contains("failures:") || text.contains("cargo test") {
        return Some(SmartFailureKind::Test);
    }
    if text.contains("failed to resolve")
        || text.contains("dependency")
        || text.contains("crate not found")
    {
        return Some(SmartFailureKind::Dependency);
    }
    if text.contains("network unreachable")
        || text.contains("connection refused")
        || text.contains("connection timed out")
    {
        return Some(SmartFailureKind::Network);
    }
    if text.contains("build failed") || text.contains("could not compile") {
        return Some(SmartFailureKind::Build);
    }
    if text.contains("git ") || text.contains("merge conflict") {
        return Some(SmartFailureKind::Git);
    }
    if text.contains("panic") || text.contains("runtime error") {
        return Some(SmartFailureKind::Runtime);
    }
    if !text.is_empty() {
        return Some(SmartFailureKind::UnknownFailure);
    }
    None
}

pub fn derive_context_needs(input: &PreprocessInput) -> Vec<SmartContextNeed> {
    let text = normalize_text(&input.user_text);
    let mut needs = Vec::new();
    if contains_any(&text, &["git", "コミット", "差分", "commit", "diff"]) {
        needs.push(SmartContextNeed::VcsStatus);
        needs.push(SmartContextNeed::VcsDiff);
    }
    if input.session_error_summary.is_some() {
        needs.push(SmartContextNeed::ErrorLines);
    }
    if contains_any(&text, &["さっき", "前の", "直前", "前に"]) {
        needs.push(SmartContextNeed::ConversationTail);
    }
    if contains_any(&text, &["前に", "覚え", "memory", "設計方針", "決めた"])
        && input.memory_enabled
    {
        needs.push(SmartContextNeed::MemoryCards);
    }
    if contains_any(&text, &["ルール", "規約", "policy", "convention"]) {
        needs.push(SmartContextNeed::ProjectRules);
    }
    needs.sort_by_key(|need| need.as_str());
    needs.dedup();
    needs
}

pub fn derive_tool_hints(input: &PreprocessInput, intent: SmartIntentClass) -> Vec<SmartToolHint> {
    let text = normalize_text(&input.user_text);
    let mut hints = Vec::new();
    if contains_any(&text, &["git", "コミット", "差分", "commit", "diff"]) {
        hints.push(SmartToolHint::GitStatus);
        hints.push(SmartToolHint::GitDiff);
    }
    if contains_any(&text, &["前に", "覚え", "memory", "設計方針", "決めた"])
        && input.memory_enabled
    {
        hints.push(SmartToolHint::MemorySearch);
    }
    if contains_any(&text, &["grep", "検索", "探して", "find"]) {
        hints.push(SmartToolHint::Grep);
    }
    if contains_any(&text, &["読んで", "read", "ファイル"]) {
        hints.push(SmartToolHint::ReadFile);
    }
    if contains_any(&text, &["一覧", "list", "ディレクトリ"]) {
        hints.push(SmartToolHint::ListDir);
    }
    if matches!(intent, SmartIntentClass::ShellCommandCandidate)
        || contains_any(&text, &["sudo", "rm ", "chmod", "curl", "wget"])
    {
        hints.push(SmartToolHint::ShellExecCandidate);
    }
    if contains_any(&text, &["会話", "さっき", "前の", "直前"]) {
        hints.push(SmartToolHint::ConversationSearch);
    }
    hints.sort_by_key(|hint| hint.as_str());
    hints.dedup();
    hints
}

pub fn compute_route_turn_required(
    mode: SmartPreprocessMode,
    intent: SmartIntentClass,
    short_circuit_allowed: bool,
    inject_hints: bool,
) -> bool {
    if intent_always_requires_route_turn(intent) {
        return true;
    }
    match mode {
        SmartPreprocessMode::Off | SmartPreprocessMode::Shadow | SmartPreprocessMode::Assist => {
            true
        }
        SmartPreprocessMode::Gate => !short_circuit_allowed || inject_hints,
    }
}

pub fn intent_always_requires_route_turn(intent: SmartIntentClass) -> bool {
    matches!(
        intent,
        SmartIntentClass::MemoryRecipeHint
            | SmartIntentClass::MemoryLookup
            | SmartIntentClass::Retry
            | SmartIntentClass::Rerun
            | SmartIntentClass::ShellCommandCandidate
            | SmartIntentClass::Ambiguous
            | SmartIntentClass::Unknown
    )
}

pub fn compute_short_circuit_allowed(
    intent: SmartIntentClass,
    gate: SmartConfidenceGate,
    safety: &SmartSafetySummary,
    config: &PreprocessConfig,
    input: &PreprocessInput,
) -> bool {
    config.mode == SmartPreprocessMode::Gate
        && gate == SmartConfidenceGate::ShortCircuitAllowed
        && config.allow_shortcuts.contains(&intent)
        && safety.is_safe_for_short_circuit()
        && config.model.is_some()
        && input.tty
        && !input.cli_overrides
        && !intent_always_requires_route_turn(intent)
}

pub fn compute_inject_hints(
    mode: SmartPreprocessMode,
    gate: SmartConfidenceGate,
    context_needs: &[SmartContextNeed],
    tool_hints: &[SmartToolHint],
    failure_kind: Option<SmartFailureKind>,
    input: &PreprocessInput,
    max_evidence_bytes: usize,
) -> bool {
    let has_wire_hints =
        !context_needs.is_empty() || !tool_hints.is_empty() || failure_kind.is_some();
    let has_bounded_summary = build_bounded_summary(input, max_evidence_bytes).is_some();
    match mode {
        SmartPreprocessMode::Off | SmartPreprocessMode::Shadow => false,
        SmartPreprocessMode::Assist => {
            gate == SmartConfidenceGate::AssistRouteTurn
                || gate == SmartConfidenceGate::ShortCircuitAllowed
                || has_wire_hints
        }
        SmartPreprocessMode::Gate => {
            if gate == SmartConfidenceGate::ShortCircuitAllowed {
                has_wire_hints || has_bounded_summary
            } else {
                gate == SmartConfidenceGate::AssistRouteTurn
                    || has_wire_hints
                    || has_bounded_summary
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn build_route_turn_hints(
    input: &PreprocessInput,
    intent: SmartIntentClass,
    inject_hints: bool,
    recent_summary: Option<String>,
    reason_codes: &[String],
    failure_kind: Option<SmartFailureKind>,
    context_needs: &[SmartContextNeed],
    tool_hints: &[SmartToolHint],
) -> SmartRouteTurnHints {
    let mut hints = SmartRouteTurnHints {
        recent_summary: if inject_hints { recent_summary } else { None },
        new_conversation: input.new_conversation,
        conversation_id: input.conversation_id.clone(),
        ..Default::default()
    };
    if !inject_hints {
        return hints;
    }
    hints.context_needs = context_needs
        .iter()
        .map(|need| need.as_str().to_string())
        .collect();
    hints.tool_hints = tool_hints
        .iter()
        .map(|hint| hint.as_str().to_string())
        .collect();
    hints.failure_kind = failure_kind.map(|kind| kind.as_str().to_string());
    hints.preprocessor_intent = Some(intent.as_str().to_string());
    hints.preprocessor_reason_codes = reason_codes.to_vec();
    hints
}

pub fn run_preprocessor(
    input: PreprocessInput,
    config: &PreprocessConfig,
) -> SmartPreprocessDecision {
    let signal_feature_count = count_signal_features(&input);
    let features = build_hashed_features(
        &input,
        config.feature_hash_buckets,
        config.feature_hash_seed,
    );
    let safety = assess_safety(&input);
    let (intent, confidence_bps, head_scores, mut reason_codes) =
        if let Some(model) = config.model.as_ref() {
            classify_with_model(&input, &features, model)
        } else {
            let (intent, confidence_bps, mut codes) = classify_local(&input, &features);
            codes.push("model_missing".into());
            (
                intent,
                confidence_bps,
                SmartHeadScores {
                    intent_bps: confidence_bps,
                    safety_bps: 0,
                    gate_bps: confidence_bps,
                },
                codes,
            )
        };
    if !input.tty {
        reason_codes.push("non_tty_conservative".into());
    }
    if input.cli_overrides {
        reason_codes.push("cli_override".into());
    }
    let gate = apply_confidence_gate(intent, confidence_bps, &safety, &head_scores, config);
    let failure_kind = infer_failure_kind(input.session_error_summary.as_deref());
    let context_needs = derive_context_needs(&input);
    let tool_hints = derive_tool_hints(&input, intent);
    let short_circuit_allowed =
        compute_short_circuit_allowed(intent, gate, &safety, config, &input);
    let inject_hints = compute_inject_hints(
        config.mode,
        gate,
        &context_needs,
        &tool_hints,
        failure_kind,
        &input,
        config.max_evidence_bytes,
    );
    let route_turn_required =
        compute_route_turn_required(config.mode, intent, short_circuit_allowed, inject_hints);
    if short_circuit_allowed && !route_turn_required {
        reason_codes.push("gate_short_circuit".into());
    }
    let recent_summary = if inject_hints {
        build_bounded_summary(&input, config.max_evidence_bytes)
    } else {
        None
    };
    let reason_codes = clamp_reason_codes(reason_codes);
    let mut route_turn_hints = build_route_turn_hints(
        &input,
        intent,
        inject_hints,
        recent_summary,
        &reason_codes,
        failure_kind,
        &context_needs,
        &tool_hints,
    );
    if inject_hints {
        route_turn_hints.confidence_bps = Some(confidence_bps);
        route_turn_hints.confidence_gate = Some(gate.as_str().to_string());
        route_turn_hints.safety_requires_approval = Some(safety.requires_approval);
    }
    let mut evidence = Vec::new();
    evidence.push(SmartEvidence {
        kind: "user_excerpt".into(),
        value: redact_for_evidence(&input.user_text, 200),
    });
    if let Some(ref summary) = route_turn_hints.recent_summary {
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
        short_circuit_allowed,
        inject_hints,
        route_turn_hints,
        safety,
        evidence,
        head_scores,
        model_version: config.model.as_ref().map(|m| m.model_version.clone()),
        feature_hash_version: FEATURE_EXTRACTOR_VERSION.to_string(),
        reason_codes,
        failure_kind,
        context_needs,
        tool_hints,
        signal_feature_count,
    }
}

pub const LOCAL_ROUTE_ESTIMATED_TOKENS_SAVED: u32 = 800;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalRouteKind {
    SimpleChat,
    ShellHelp,
    #[serde(alias = "git_inspect")]
    ToolBackedInspection,
    OutputStyleRequest,
    CodeReviewContextSelection,
}

impl LocalRouteKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SimpleChat => "simple_chat",
            Self::ShellHelp => "shell_help",
            Self::ToolBackedInspection => "tool_backed_inspection",
            Self::OutputStyleRequest => "output_style_request",
            Self::CodeReviewContextSelection => "code_review_context_selection",
        }
    }

    /// Local fast path に必要な built-in tool capability（いずれか 1 つで可）。
    pub fn required_tool_capabilities(self) -> &'static [LocalToolHint] {
        match self {
            Self::ToolBackedInspection => &[LocalToolHint::GitStatus, LocalToolHint::GitDiff],
            _ => &[],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalOutputStyle {
    Default,
    Concise,
    Expanded,
    Checklist,
    CodeFirst,
    ReviewFirst,
}

impl LocalOutputStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Concise => "concise",
            Self::Expanded => "expanded",
            Self::Checklist => "checklist",
            Self::CodeFirst => "code_first",
            Self::ReviewFirst => "review_first",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalToolHint {
    GitStatus,
    GitDiff,
    Grep,
    ReadFile,
    ListDir,
}

impl LocalToolHint {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GitStatus => "git_status",
            Self::GitDiff => "git_diff",
            Self::Grep => "grep",
            Self::ReadFile => "read_file",
            Self::ListDir => "list_dir",
        }
    }

    pub fn runtime_tool_name(self) -> &'static str {
        self.as_str()
    }

    pub fn from_smart_tool_hint(hint: SmartToolHint) -> Option<Self> {
        match hint {
            SmartToolHint::GitStatus => Some(Self::GitStatus),
            SmartToolHint::GitDiff => Some(Self::GitDiff),
            SmartToolHint::Grep => Some(Self::Grep),
            SmartToolHint::ReadFile => Some(Self::ReadFile),
            SmartToolHint::ListDir => Some(Self::ListDir),
            SmartToolHint::ShellExecCandidate
            | SmartToolHint::MemorySearch
            | SmartToolHint::ConversationSearch => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalRouteDecision {
    pub route_kind: LocalRouteKind,
    pub enabled_tools: Vec<LocalToolHint>,
    pub context_needs: Vec<SmartContextNeed>,
    pub output_style: LocalOutputStyle,
    pub fallback_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    pub source_intent: SmartIntentClass,
    pub confidence_bps: u16,
    pub estimated_tokens_saved: u32,
}

pub fn local_tool_for_context_need(need: SmartContextNeed) -> Option<LocalToolHint> {
    match need {
        SmartContextNeed::VcsStatus => Some(LocalToolHint::GitStatus),
        SmartContextNeed::VcsDiff => Some(LocalToolHint::GitDiff),
        _ => None,
    }
}

pub fn has_required_local_tool_capabilities(
    route_kind: LocalRouteKind,
    enabled_tools: &[LocalToolHint],
    context_needs: &[SmartContextNeed],
) -> bool {
    match route_kind {
        LocalRouteKind::ToolBackedInspection => {
            let mut required: Vec<LocalToolHint> = context_needs
                .iter()
                .filter_map(|need| local_tool_for_context_need(*need))
                .collect();
            if required.is_empty() {
                required.extend_from_slice(route_kind.required_tool_capabilities());
            }
            required.sort_by_key(|tool| tool.as_str());
            required.dedup();
            required
                .iter()
                .all(|required_tool| enabled_tools.contains(required_tool))
        }
        _ => true,
    }
}

fn set_local_route_fallback_reason(
    fallback_required: &mut bool,
    fallback_reason: &mut Option<String>,
    reason: &'static str,
) {
    *fallback_required = true;
    if fallback_reason.is_none() {
        *fallback_reason = Some(reason.to_string());
    }
}

pub fn build_local_route_context_summary(context_needs: &[SmartContextNeed]) -> Option<String> {
    if context_needs.is_empty() {
        return None;
    }
    let labels: Vec<&str> = context_needs.iter().map(|need| need.as_str()).collect();
    Some(format!("local_context_needs: {}", labels.join(",")))
}

pub fn local_output_style_system_hint(style: LocalOutputStyle) -> Option<&'static str> {
    match style {
        LocalOutputStyle::Default => None,
        LocalOutputStyle::Concise => {
            Some("Respond concisely. Prefer short paragraphs and minimal preamble.")
        }
        LocalOutputStyle::Expanded => Some("Provide an expanded explanation with useful detail."),
        LocalOutputStyle::Checklist => {
            Some("Respond as a checklist with short actionable bullets.")
        }
        LocalOutputStyle::CodeFirst => Some("Lead with code or command examples before prose."),
        LocalOutputStyle::ReviewFirst => {
            Some("Lead with review findings, risks, and recommendations.")
        }
    }
}

pub fn intent_supports_local_route(intent: SmartIntentClass) -> bool {
    matches!(
        intent,
        SmartIntentClass::SimpleChat | SmartIntentClass::Inspect
    )
}

pub fn derive_local_route_kind(intent: SmartIntentClass, text: &str) -> Option<LocalRouteKind> {
    let text = normalize_text(text);
    match intent {
        SmartIntentClass::SimpleChat => Some(LocalRouteKind::SimpleChat),
        SmartIntentClass::Inspect => {
            if contains_any(
                &text,
                &["git", "コミット", "差分", "commit", "diff", "status"],
            ) {
                Some(LocalRouteKind::ToolBackedInspection)
            } else if contains_any(
                &text,
                &["shell", "bash", "zsh", "コマンド", "使い方", "help"],
            ) {
                Some(LocalRouteKind::ShellHelp)
            } else if contains_any(
                &text,
                &["簡潔", "箇条書き", "短く", "concise", "checklist", "output"],
            ) {
                Some(LocalRouteKind::OutputStyleRequest)
            } else if contains_any(&text, &["review", "レビュー", "コード", "refactor"]) {
                Some(LocalRouteKind::CodeReviewContextSelection)
            } else {
                None
            }
        }
        _ => None,
    }
}

pub fn derive_local_output_style(text: &str) -> LocalOutputStyle {
    let text = normalize_text(text);
    if contains_any(&text, &["簡潔", "短く", "concise"]) {
        LocalOutputStyle::Concise
    } else if contains_any(&text, &["箇条書き", "checklist"]) {
        LocalOutputStyle::Checklist
    } else if contains_any(&text, &["詳しく", "expanded", "詳細"]) {
        LocalOutputStyle::Expanded
    } else if contains_any(&text, &["コード", "code first"]) {
        LocalOutputStyle::CodeFirst
    } else if contains_any(&text, &["review", "レビュー"]) {
        LocalOutputStyle::ReviewFirst
    } else {
        LocalOutputStyle::Default
    }
}

pub fn project_safe_local_tools(tool_hints: &[SmartToolHint]) -> Vec<LocalToolHint> {
    let mut out = Vec::new();
    for hint in tool_hints {
        if let Some(local) = LocalToolHint::from_smart_tool_hint(*hint) {
            out.push(local);
        }
    }
    out.sort_by_key(|hint| hint.as_str());
    out.dedup();
    out
}

pub fn clamp_local_tools_to_allowlist(
    tools: Vec<LocalToolHint>,
    allowlist: &[String],
) -> Vec<LocalToolHint> {
    if allowlist.is_empty() {
        return Vec::new();
    }
    let allowed: std::collections::HashSet<&str> = allowlist.iter().map(String::as_str).collect();
    tools
        .into_iter()
        .filter(|tool| allowed.contains(tool.runtime_tool_name()))
        .collect()
}

pub fn derive_local_route_decision(
    decision: &SmartPreprocessDecision,
    user_text: &str,
    config: &PreprocessConfig,
    cli_tool_allowlist: &[String],
    tty: bool,
    cli_overrides: bool,
) -> Option<LocalRouteDecision> {
    if config.mode != SmartPreprocessMode::Gate {
        return None;
    }
    if !intent_supports_local_route(decision.intent) {
        return None;
    }
    if intent_always_requires_route_turn(decision.intent) {
        return None;
    }
    let route_kind = derive_local_route_kind(decision.intent, user_text)?;
    let mut fallback_required = false;
    let mut fallback_reason = None;
    if !decision.safety.is_safe_for_short_circuit() {
        set_local_route_fallback_reason(&mut fallback_required, &mut fallback_reason, "unsafe");
    }
    if decision.confidence_bps < config.route_turn_threshold_bps {
        set_local_route_fallback_reason(
            &mut fallback_required,
            &mut fallback_reason,
            "low_confidence",
        );
    }
    if decision.tool_hints.iter().any(|hint| {
        matches!(
            hint,
            SmartToolHint::MemorySearch | SmartToolHint::ConversationSearch
        )
    }) {
        set_local_route_fallback_reason(
            &mut fallback_required,
            &mut fallback_reason,
            "memory_or_conversation_tool",
        );
    }
    if !tty {
        set_local_route_fallback_reason(&mut fallback_required, &mut fallback_reason, "non_tty");
    }
    if cli_overrides {
        set_local_route_fallback_reason(
            &mut fallback_required,
            &mut fallback_reason,
            "cli_override",
        );
    }
    let context_needs: Vec<_> = decision
        .context_needs
        .iter()
        .filter(|need| !matches!(need, SmartContextNeed::MemoryCards))
        .copied()
        .collect();
    let enabled_tools = clamp_local_tools_to_allowlist(
        project_safe_local_tools(&decision.tool_hints),
        cli_tool_allowlist,
    );
    if !has_required_local_tool_capabilities(route_kind, &enabled_tools, &context_needs) {
        set_local_route_fallback_reason(
            &mut fallback_required,
            &mut fallback_reason,
            "missing_required_local_tool",
        );
    }
    Some(LocalRouteDecision {
        route_kind,
        enabled_tools,
        context_needs,
        output_style: derive_local_output_style(user_text),
        fallback_required,
        fallback_reason,
        source_intent: decision.intent,
        confidence_bps: decision.confidence_bps,
        estimated_tokens_saved: if fallback_required {
            0
        } else {
            LOCAL_ROUTE_ESTIMATED_TOKENS_SAVED
        },
    })
}

pub fn should_use_local_route(
    decision: &SmartPreprocessDecision,
    local: &LocalRouteDecision,
    config: &PreprocessConfig,
    tty: bool,
    cli_overrides: bool,
) -> bool {
    config.mode == SmartPreprocessMode::Gate
        && !local.fallback_required
        && decision.safety.is_safe_for_short_circuit()
        && decision.confidence_bps >= config.route_turn_threshold_bps
        && decision.model_version.as_deref() == Some(DEFAULT_MODEL_VERSION)
        && decision.feature_hash_version == FEATURE_EXTRACTOR_VERSION
        && tty
        && !cli_overrides
        && !intent_always_requires_route_turn(decision.intent)
}

pub fn should_short_circuit(decision: &SmartPreprocessDecision) -> bool {
    decision.mode == SmartPreprocessMode::Gate
        && decision.short_circuit_allowed
        && !decision.route_turn_required
        && !decision.inject_hints
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
            cli_overrides: false,
            route_metadata: RouteMetadataInput::default(),
        }
    }

    fn bundled_model() -> PreprocessorModel {
        let raw = include_str!("../../resources/smart_preprocessor_model.json");
        let file: serde_json::Value = serde_json::from_str(raw).expect("json");
        let intent = file["heads"]["intent"]
            .as_object()
            .expect("intent heads")
            .iter()
            .map(|(name, head)| {
                let intent = match name.as_str() {
                    "simple_chat" => SmartIntentClass::SimpleChat,
                    "inspect" => SmartIntentClass::Inspect,
                    "debug" => SmartIntentClass::Debug,
                    "memory_lookup" => SmartIntentClass::MemoryLookup,
                    "shell_command_candidate" => SmartIntentClass::ShellCommandCandidate,
                    "retry" => SmartIntentClass::Retry,
                    "rerun" => SmartIntentClass::Rerun,
                    "ambiguous" => SmartIntentClass::Ambiguous,
                    "unknown" => SmartIntentClass::Unknown,
                    other => panic!("unknown intent {other}"),
                };
                let bias = head["bias"].as_f64().expect("bias") as f32;
                let weights = head["features"]
                    .as_object()
                    .expect("features")
                    .iter()
                    .map(|(name, weight)| {
                        (
                            hash_feature(name, 262144, 17),
                            weight.as_f64().expect("weight") as f32,
                        )
                    })
                    .collect();
                (intent, SparseLogisticHead { bias, weights })
            })
            .collect();
        let safety = &file["heads"]["safety"];
        let gate = &file["heads"]["gate"];
        let read_head = |node: &serde_json::Value| SparseLogisticHead {
            bias: node["bias"].as_f64().expect("bias") as f32,
            weights: node["features"]
                .as_object()
                .expect("features")
                .iter()
                .map(|(name, weight)| {
                    (
                        hash_feature(name, 262144, 17),
                        weight.as_f64().expect("weight") as f32,
                    )
                })
                .collect(),
        };
        PreprocessorModel {
            model_version: file["model_version"].as_str().unwrap().into(),
            feature_extractor_version: file["feature_extractor_version"].as_str().unwrap().into(),
            intent_heads: intent,
            safety_head: read_head(safety),
            gate_head: read_head(gate),
        }
    }

    fn config_with_model(mode: SmartPreprocessMode) -> PreprocessConfig {
        PreprocessConfig {
            mode,
            model: Some(bundled_model()),
            ..PreprocessConfig::default()
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
    fn feature_hash_bucket_change_alters_index() {
        let name = "user_ngram:bucket-regression";
        let base = hash_feature(name, 4096, 17);
        let mut found_diff = false;
        for buckets in [2u32, 3, 5, 7, 4095, 8192, 16384] {
            if hash_feature(name, buckets, 17) != base {
                found_diff = true;
                break;
            }
        }
        assert!(
            found_diff,
            "expected some bucket size to change hash index for {name}"
        );
    }

    #[test]
    fn fix_error_route_golden() {
        let mut input = sample_input("さっきのエラーを直して");
        input
            .session_error_summary
            .get_or_insert("test failed".into());
        let decision = run_preprocessor(input, &config_with_model(SmartPreprocessMode::Shadow));
        assert!(matches!(
            decision.intent,
            SmartIntentClass::Debug | SmartIntentClass::Ambiguous
        ));
        assert!(decision.route_turn_required);
    }

    #[test]
    fn git_assist_golden() {
        let input = sample_input("この差分をコミット単位に分けたい");
        let decision = run_preprocessor(input, &config_with_model(SmartPreprocessMode::Shadow));
        assert_eq!(decision.intent, SmartIntentClass::Inspect);
    }

    #[test]
    fn memory_query_golden() {
        let input = sample_input("前に決めた設計方針を見たい");
        let decision = run_preprocessor(input, &config_with_model(SmartPreprocessMode::Shadow));
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
        let head_scores = SmartHeadScores {
            intent_bps: 8600,
            safety_bps: 1000,
            gate_bps: 8600,
        };
        let mut gate_cfg = PreprocessConfig::default();
        gate_cfg.mode = SmartPreprocessMode::Gate;
        assert_eq!(
            apply_confidence_gate(
                SmartIntentClass::SimpleChat,
                8600,
                &safety,
                &head_scores,
                &gate_cfg
            ),
            SmartConfidenceGate::ShortCircuitAllowed
        );
        let cfg = PreprocessConfig::default();
        assert_eq!(
            apply_confidence_gate(
                SmartIntentClass::SimpleChat,
                8600,
                &safety,
                &head_scores,
                &cfg
            ),
            SmartConfidenceGate::AssistRouteTurn
        );
        assert_eq!(
            apply_confidence_gate(
                SmartIntentClass::SimpleChat,
                4000,
                &safety,
                &head_scores,
                &cfg
            ),
            SmartConfidenceGate::ForceRouteTurn
        );
    }

    #[test]
    fn dangerous_input_forces_route_turn() {
        let input = sample_input("sudo rm -rf /tmp/foo");
        let cfg = config_with_model(SmartPreprocessMode::Gate);
        let decision = run_preprocessor(input, &cfg);
        assert!(decision.route_turn_required);
        assert!(!decision.safety.is_safe_for_short_circuit());
    }

    #[test]
    fn serde_roundtrip_decision() {
        let input = sample_input("hello");
        let decision = run_preprocessor(input, &config_with_model(SmartPreprocessMode::Shadow));
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
            user_text: "hello".into(),
            command: Some("ask".into()),
            tty: true,
            new_conversation: true,
            conversation_id: None,
            memory_enabled: true,
            history_tail_summary: None,
            session_error_summary: None,
            cli_overrides: false,
            route_metadata: RouteMetadataInput::default(),
        };
        let mut cfg = PreprocessConfig::default();
        cfg.mode = SmartPreprocessMode::Gate;
        cfg.model = None;
        let decision = run_preprocessor(input, &cfg);
        assert!(decision.route_turn_required);
    }

    #[test]
    fn classifier_model_weights_affect_inference() {
        let input = sample_input("hello");
        let mut high = config_with_model(SmartPreprocessMode::Gate);
        let mut low = config_with_model(SmartPreprocessMode::Gate);
        high.model.as_mut().expect("model").gate_head.bias = 5.0;
        low.model.as_mut().expect("model").gate_head.bias = -5.0;
        let decision_high = run_preprocessor(input.clone(), &high);
        let decision_low = run_preprocessor(input, &low);
        assert_eq!(decision_high.intent, SmartIntentClass::SimpleChat);
        assert!(decision_high.confidence_bps > decision_low.confidence_bps);
    }

    #[test]
    fn simple_chat_confidence_reaches_gate_threshold() {
        let input = sample_input("hello");
        let cfg = config_with_model(SmartPreprocessMode::Gate);
        let decision = run_preprocessor(input, &cfg);
        assert_eq!(decision.intent, SmartIntentClass::SimpleChat);
        assert!(
            decision.confidence_bps >= cfg.route_turn_threshold_bps,
            "confidence {} must reach threshold {}",
            decision.confidence_bps,
            cfg.route_turn_threshold_bps
        );
    }

    #[test]
    fn signal_feature_count_matches_enabled_boolean_signals() {
        let input = sample_input("hello");
        let cfg = PreprocessConfig::default();
        let decision = run_preprocessor(input.clone(), &cfg);
        assert_eq!(decision.signal_feature_count, count_signal_features(&input));
    }

    #[test]
    fn cli_override_blocks_gate_short_circuit() {
        let mut input = sample_input("hello");
        input.cli_overrides = true;
        let cfg = config_with_model(SmartPreprocessMode::Gate);
        let decision = run_preprocessor(input, &cfg);
        assert!(decision.route_turn_required);
    }

    #[test]
    fn memory_write_blocks_gate_short_circuit() {
        let input = sample_input("メモして");
        let cfg = config_with_model(SmartPreprocessMode::Gate);
        let decision = run_preprocessor(input, &cfg);
        assert!(decision.route_turn_required);
        assert!(decision.safety.contains_write_risk);
    }

    #[test]
    fn feature_hash_redacts_secrets_before_hashing() {
        let secret_input = PreprocessInput {
            user_text: "token=ghp_abc".into(),
            command: Some("ask".into()),
            tty: true,
            new_conversation: true,
            conversation_id: None,
            memory_enabled: true,
            history_tail_summary: None,
            session_error_summary: None,
            cli_overrides: false,
            route_metadata: RouteMetadataInput::default(),
        };
        let clean_input = sample_input("hello");
        let secret_features = build_hashed_features(&secret_input, 4096, 17);
        let clean_features = build_hashed_features(&clean_input, 4096, 17);
        assert_ne!(secret_features, clean_features);
        let redacted = redact_for_features(&secret_input.user_text, 200);
        assert!(redacted.contains("[redacted]"));
    }

    #[test]
    fn high_safety_head_blocks_short_circuit() {
        let safety = SmartSafetySummary {
            requires_approval: false,
            contains_secret_risk: false,
            contains_write_risk: false,
            contains_network_risk: false,
        };
        let head_scores = SmartHeadScores {
            intent_bps: 9000,
            safety_bps: 9000,
            gate_bps: 9000,
        };
        let mut gate_cfg = PreprocessConfig::default();
        gate_cfg.mode = SmartPreprocessMode::Gate;
        assert_eq!(
            apply_confidence_gate(
                SmartIntentClass::SimpleChat,
                9000,
                &safety,
                &head_scores,
                &gate_cfg
            ),
            SmartConfidenceGate::ForceRouteTurn
        );
    }

    #[test]
    fn permission_denied_sets_failure_kind_permission() {
        let mut input = sample_input("さっきのエラーを直して");
        input.session_error_summary = Some("permission denied: /etc/shadow".into());
        let decision = run_preprocessor(input, &config_with_model(SmartPreprocessMode::Shadow));
        assert_eq!(decision.failure_kind, Some(SmartFailureKind::Permission));
    }

    #[test]
    fn context_needs_and_tool_hints_are_populated() {
        let input = sample_input("前に決めた設計方針を見たい");
        let decision = run_preprocessor(input, &config_with_model(SmartPreprocessMode::Shadow));
        assert!(decision
            .context_needs
            .contains(&SmartContextNeed::MemoryCards));
        assert!(decision
            .context_needs
            .contains(&SmartContextNeed::ConversationTail));
        assert!(decision.tool_hints.contains(&SmartToolHint::MemorySearch));

        let git_input = sample_input("この差分をコミット単位に分けたい");
        let git_decision =
            run_preprocessor(git_input, &config_with_model(SmartPreprocessMode::Shadow));
        assert!(git_decision
            .context_needs
            .contains(&SmartContextNeed::VcsStatus));
        assert!(git_decision
            .context_needs
            .contains(&SmartContextNeed::VcsDiff));
        assert!(git_decision.tool_hints.contains(&SmartToolHint::GitStatus));
        assert!(git_decision.tool_hints.contains(&SmartToolHint::GitDiff));
    }

    #[test]
    fn assist_and_route_turn_thresholds_are_distinct() {
        let safety = SmartSafetySummary {
            requires_approval: false,
            contains_secret_risk: false,
            contains_write_risk: false,
            contains_network_risk: false,
        };
        let head_scores = SmartHeadScores {
            intent_bps: 7000,
            safety_bps: 1000,
            gate_bps: 7000,
        };
        let cfg = PreprocessConfig::default();
        assert_eq!(
            apply_confidence_gate(
                SmartIntentClass::SimpleChat,
                7000,
                &safety,
                &head_scores,
                &cfg
            ),
            SmartConfidenceGate::AssistRouteTurn
        );
        let mut gate_cfg = PreprocessConfig::default();
        gate_cfg.mode = SmartPreprocessMode::Gate;
        assert_eq!(
            apply_confidence_gate(
                SmartIntentClass::SimpleChat,
                7000,
                &safety,
                &head_scores,
                &gate_cfg
            ),
            SmartConfidenceGate::AssistRouteTurn
        );
        assert!(!should_short_circuit(&SmartPreprocessDecision {
            version: 1,
            mode: SmartPreprocessMode::Gate,
            intent: SmartIntentClass::SimpleChat,
            confidence_bps: 7000,
            gate: SmartConfidenceGate::AssistRouteTurn,
            route_turn_required: true,
            short_circuit_allowed: false,
            inject_hints: true,
            route_turn_hints: SmartRouteTurnHints::default(),
            safety: safety.clone(),
            evidence: Vec::new(),
            head_scores: head_scores.clone(),
            model_version: Some(DEFAULT_MODEL_VERSION.to_string()),
            feature_hash_version: FEATURE_EXTRACTOR_VERSION.to_string(),
            reason_codes: Vec::new(),
            failure_kind: None,
            context_needs: Vec::new(),
            tool_hints: Vec::new(),
            signal_feature_count: 0,
        }));
    }

    #[test]
    fn assist_threshold_injects_error_summary_hint() {
        let mut input = sample_input("さっきのエラーを直して");
        input.session_error_summary = Some("test failed: foo".into());
        let mut cfg = config_with_model(SmartPreprocessMode::Assist);
        cfg.assist_threshold_bps = DEFAULT_HINT_THRESHOLD_BPS;
        let decision = run_preprocessor(input, &cfg);
        assert_eq!(decision.gate, SmartConfidenceGate::AssistRouteTurn);
        let summary = decision
            .route_turn_hints
            .recent_summary
            .expect("recent_summary");
        assert!(summary.contains("session_error"));
        assert!(summary.contains("test failed"));
    }

    #[test]
    fn session_error_feature_uses_session_error_prefix() {
        let mut input = sample_input("fix error");
        input.session_error_summary = Some("permission denied".into());
        let features = build_hashed_features(&input, 4096, 17);
        assert!(!features.is_empty());
        let names = vec!["session_error_ngram:pe".to_string()];
        let idx = hash_feature(&names[0], 4096, 17);
        assert!(features.iter().any(|f| f.index == idx));
    }

    #[test]
    fn clamp_reason_codes_limits_count_and_length() {
        let codes: Vec<String> = (0..20).map(|i| format!("code_{i}")).collect();
        let long = "x".repeat(80);
        let mut with_long = codes;
        with_long.push(long);
        let clamped = clamp_reason_codes(with_long);
        assert_eq!(clamped.len(), MAX_REASON_CODES);
        assert!(clamped.iter().all(|code| code.len() <= MAX_REASON_CODE_LEN));
    }

    #[test]
    fn smart_route_turn_hints_extend_with_wire_subset() {
        let hints = build_route_turn_hints(
            &sample_input("git diff を見て"),
            SmartIntentClass::Inspect,
            true,
            None,
            &["git_context".into()],
            None,
            &[SmartContextNeed::VcsStatus, SmartContextNeed::VcsDiff],
            &[SmartToolHint::GitStatus],
        );
        assert_eq!(
            hints.context_needs,
            vec!["vcs_status".to_string(), "vcs_diff".to_string()]
        );
        assert_eq!(hints.tool_hints, vec!["git_status".to_string()]);
        assert_eq!(hints.preprocessor_intent.as_deref(), Some("inspect"));
        assert!(hints.has_route_turn_hint_payload());
    }

    #[test]
    fn three_axis_gate_decision_is_independent() {
        let input = sample_input("hello");
        let safety = assess_safety(&input);
        let cfg = config_with_model(SmartPreprocessMode::Gate);
        let gate = SmartConfidenceGate::ShortCircuitAllowed;
        let short = compute_short_circuit_allowed(
            SmartIntentClass::SimpleChat,
            gate,
            &safety,
            &cfg,
            &input,
        );
        assert!(short);
        assert!(!compute_inject_hints(
            SmartPreprocessMode::Gate,
            gate,
            &[],
            &[],
            None,
            &input,
            cfg.max_evidence_bytes
        ));
        assert!(!compute_route_turn_required(
            SmartPreprocessMode::Gate,
            SmartIntentClass::SimpleChat,
            short,
            false
        ));
        assert!(compute_route_turn_required(
            SmartPreprocessMode::Gate,
            SmartIntentClass::SimpleChat,
            short,
            true
        ));
        let git_needs = derive_context_needs(&sample_input("git diff"));
        assert!(compute_inject_hints(
            SmartPreprocessMode::Gate,
            SmartConfidenceGate::ShortCircuitAllowed,
            &git_needs,
            &[],
            None,
            &sample_input("git diff"),
            cfg.max_evidence_bytes
        ));
    }

    #[test]
    fn memory_lookup_stays_route_turn_required_but_allows_hint_injection() {
        let mut input = sample_input("前に決めた設計方針を教えて");
        input.memory_enabled = true;
        let mut cfg = config_with_model(SmartPreprocessMode::Assist);
        cfg.assist_threshold_bps = 10_000;
        let decision = run_preprocessor(input, &cfg);
        assert_eq!(decision.intent, SmartIntentClass::MemoryLookup);
        assert!(decision.route_turn_required);
        assert!(decision.inject_hints);
        assert!(!decision.context_needs.is_empty() || !decision.tool_hints.is_empty());
        assert!(decision.route_turn_hints.has_route_turn_hint_payload());
    }

    #[test]
    fn local_route_decision_is_deterministic() {
        let input = sample_input("git diff を見て");
        let cfg = config_with_model(SmartPreprocessMode::Gate);
        let decision = run_preprocessor(input.clone(), &cfg);
        let allowlist = vec![
            "git_status".into(),
            "git_diff".into(),
            "grep".into(),
            "read_file".into(),
            "list_dir".into(),
        ];
        let first =
            derive_local_route_decision(&decision, &input.user_text, &cfg, &allowlist, true, false);
        let second =
            derive_local_route_decision(&decision, &input.user_text, &cfg, &allowlist, true, false);
        assert_eq!(first, second);
        let local = first.expect("local route");
        assert_eq!(local.route_kind, LocalRouteKind::ToolBackedInspection);
        assert!(!local.enabled_tools.is_empty());
    }

    #[test]
    fn local_route_excludes_memory_from_fast_path() {
        let input = sample_input("前に決めた設計方針を教えて");
        let cfg = config_with_model(SmartPreprocessMode::Gate);
        let decision = run_preprocessor(input.clone(), &cfg);
        assert_eq!(decision.intent, SmartIntentClass::MemoryLookup);
        assert!(
            derive_local_route_decision(&decision, &input.user_text, &cfg, &[], true, false)
                .is_none()
        );
    }

    #[test]
    fn local_route_kind_derivation_covers_phase_targets() {
        assert_eq!(
            derive_local_route_kind(SmartIntentClass::SimpleChat, "hello"),
            Some(LocalRouteKind::SimpleChat)
        );
        assert_eq!(
            derive_local_route_kind(SmartIntentClass::Inspect, "bash の使い方を教えて"),
            Some(LocalRouteKind::ShellHelp)
        );
        assert_eq!(
            derive_local_route_kind(SmartIntentClass::Inspect, "git diff を見て"),
            Some(LocalRouteKind::ToolBackedInspection)
        );
        assert_eq!(
            derive_local_route_kind(SmartIntentClass::Inspect, "箇条書きで答えて"),
            Some(LocalRouteKind::OutputStyleRequest)
        );
        assert_eq!(
            derive_local_route_kind(SmartIntentClass::Inspect, "このコードをレビューして"),
            Some(LocalRouteKind::CodeReviewContextSelection)
        );
    }

    #[test]
    fn local_route_context_and_output_style_hints_are_bounded() {
        let summary = build_local_route_context_summary(&[
            SmartContextNeed::VcsStatus,
            SmartContextNeed::VcsDiff,
        ])
        .expect("summary");
        assert!(summary.contains("vcs_status"));
        assert!(summary.contains("vcs_diff"));
        assert!(local_output_style_system_hint(LocalOutputStyle::Default).is_none());
        assert!(local_output_style_system_hint(LocalOutputStyle::Concise).is_some());
    }

    #[test]
    fn tool_backed_inspection_requires_vcs_tool_capability_in_allowlist() {
        let input = sample_input("git diff を見て");
        let cfg = config_with_model(SmartPreprocessMode::Gate);
        let decision = run_preprocessor(input.clone(), &cfg);
        let without_tools = derive_local_route_decision(
            &decision,
            &input.user_text,
            &cfg,
            &["grep".into()],
            true,
            false,
        )
        .expect("local route");
        assert_eq!(
            without_tools.route_kind,
            LocalRouteKind::ToolBackedInspection
        );
        assert!(without_tools.fallback_required);
        assert_eq!(
            without_tools.fallback_reason.as_deref(),
            Some("missing_required_local_tool")
        );

        let with_git_tools = derive_local_route_decision(
            &decision,
            &input.user_text,
            &cfg,
            &["git_status".into(), "git_diff".into()],
            true,
            false,
        )
        .expect("local route");
        assert!(!with_git_tools.fallback_required);
        assert!(with_git_tools.fallback_reason.is_none());
        assert!(!with_git_tools.enabled_tools.is_empty());

        let partial_git_status = derive_local_route_decision(
            &decision,
            &input.user_text,
            &cfg,
            &["git_status".into()],
            true,
            false,
        )
        .expect("local route");
        assert!(
            partial_git_status.fallback_required,
            "vcs context needs require matching tool capabilities"
        );
        assert_eq!(
            partial_git_status.fallback_reason.as_deref(),
            Some("missing_required_local_tool")
        );

        assert!(!has_required_local_tool_capabilities(
            LocalRouteKind::ToolBackedInspection,
            &[LocalToolHint::GitStatus],
            &[],
        ));
        assert!(has_required_local_tool_capabilities(
            LocalRouteKind::ToolBackedInspection,
            &[LocalToolHint::GitStatus, LocalToolHint::GitDiff],
            &[],
        ));
    }

    #[test]
    fn smart_context_need_deserializes_legacy_git_aliases() {
        let status: SmartContextNeed = serde_json::from_str("\"git_status\"").expect("status");
        let diff: SmartContextNeed = serde_json::from_str("\"git_diff\"").expect("diff");
        assert_eq!(status, SmartContextNeed::VcsStatus);
        assert_eq!(diff, SmartContextNeed::VcsDiff);
    }

    #[test]
    fn route_turn_hints_include_confidence_fields_when_injected() {
        let input = sample_input("git diff を見て");
        let cfg = config_with_model(SmartPreprocessMode::Assist);
        let decision = run_preprocessor(input, &cfg);
        assert!(decision.inject_hints);
        let hints = decision.route_turn_hints;
        assert_eq!(hints.confidence_bps, Some(decision.confidence_bps));
        assert_eq!(
            hints.confidence_gate.as_deref(),
            Some(decision.gate.as_str())
        );
        assert_eq!(
            hints.safety_requires_approval,
            Some(decision.safety.requires_approval)
        );
    }
}
