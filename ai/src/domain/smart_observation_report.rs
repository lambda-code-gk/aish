//! Smart Preprocessor observation の read model・集計・表示。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{append_env_line, append_tsv_row, OutputFormat};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SmartObservationLine {
    pub timestamp_ms: Option<u64>,
    pub ai_session_id: Option<String>,
    pub mode: Option<String>,
    pub intent: Option<String>,
    pub confidence_bps: Option<u16>,
    pub gate: Option<String>,
    pub decision_path: Option<String>,
    pub route_turn_used: Option<bool>,
    pub route_turn_required: Option<bool>,
    pub short_circuit_allowed: Option<bool>,
    pub inject_hints: Option<bool>,
    pub route_turn_hints_present: Option<bool>,
    pub route_turn_hints_injected: Option<bool>,
    pub fallback_reason: Option<String>,
    pub failure_kind: Option<String>,
    pub local_route_kind: Option<String>,
    pub local_route_used: Option<bool>,
    pub route_turn_skipped_count: Option<u64>,
    pub route_turn_fallback_count: Option<u64>,
    pub local_route_latency_ms: Option<u64>,
    pub route_turn_latency_ms: Option<u64>,
    pub agent_turn_latency_ms: Option<u64>,
    pub total_turn_latency_ms: Option<u64>,
    pub estimated_tokens_saved: Option<u64>,
    pub llm_call_count_estimated: Option<u64>,
    pub llm_call_sites: Option<Vec<String>>,
    pub context_needs: Option<Vec<String>>,
    pub tool_hints: Option<Vec<String>>,
    pub reason_codes: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LatencySummary {
    pub sample_count: usize,
    pub avg_ms: f64,
    pub p50_ms: u64,
    pub p95_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LatencyStats {
    pub local_route: LatencySummary,
    pub route_turn: LatencySummary,
    pub agent_turn: LatencySummary,
    pub total_turn: LatencySummary,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SmartObservationStats {
    pub total_records: usize,
    pub valid_records: usize,
    pub invalid_lines: usize,
    pub first_timestamp_ms: Option<u64>,
    pub last_timestamp_ms: Option<u64>,
    pub by_mode: BTreeMap<String, usize>,
    pub by_intent: BTreeMap<String, usize>,
    pub by_gate: BTreeMap<String, usize>,
    pub by_decision_path: BTreeMap<String, usize>,
    pub by_fallback_reason: BTreeMap<String, usize>,
    pub by_failure_kind: BTreeMap<String, usize>,
    pub by_local_route_kind: BTreeMap<String, usize>,
    pub route_turn_used_count: usize,
    pub route_turn_skipped_count: u64,
    pub route_turn_fallback_count: u64,
    pub local_route_used_count: usize,
    pub route_turn_hints_present_count: usize,
    pub route_turn_hints_injected_count: usize,
    pub short_circuit_allowed_count: usize,
    pub inject_hints_count: usize,
    pub estimated_tokens_saved_sum: u64,
    pub latency: LatencyStats,
    pub llm_call_count_estimated_sum: u64,
    pub llm_call_sites: BTreeMap<String, usize>,
    pub context_needs: BTreeMap<String, usize>,
    pub tool_hints: BTreeMap<String, usize>,
    pub reason_codes: BTreeMap<String, usize>,
}

impl SmartObservationStats {
    pub fn from_records(records: &[SmartObservationLine], total: usize, invalid: usize) -> Self {
        let mut acc = Accumulator::default();
        for record in records {
            acc.add(record);
        }
        acc.finish(records.len(), total, invalid)
    }

    pub fn render(&self, format: OutputFormat) -> Result<String, serde_json::Error> {
        if format == OutputFormat::Json {
            return serde_json::to_string_pretty(self).map(|value| value + "\n");
        }
        let value = serde_json::to_value(self)?;
        let mut fields = Vec::new();
        flatten("", &value, &mut fields);
        let mut out = String::new();
        for (key, value) in fields {
            if format == OutputFormat::Env {
                append_env_line(&mut out, &format!("SMART_{}", env_key(&key)), &cell(&value));
            } else {
                append_tsv_row(&mut out, &key, &cell(&value));
            }
        }
        Ok(out)
    }
}

#[derive(Default)]
struct Accumulator {
    first: Option<u64>,
    last: Option<u64>,
    maps: [BTreeMap<String, usize>; 11],
    route_used: usize,
    skipped: u64,
    fallback: u64,
    local_used: usize,
    hints_present: usize,
    hints_injected: usize,
    short_circuit: usize,
    inject_hints: usize,
    tokens: u64,
    llm_calls: u64,
    latencies: [Vec<u64>; 4],
}

impl Accumulator {
    fn add(&mut self, r: &SmartObservationLine) {
        if let Some(timestamp) = r.timestamp_ms {
            self.first = Some(self.first.map_or(timestamp, |value| value.min(timestamp)));
            self.last = Some(self.last.map_or(timestamp, |value| value.max(timestamp)));
        }
        for (index, value) in [
            r.mode.as_deref(),
            r.intent.as_deref(),
            r.gate.as_deref(),
            r.decision_path.as_deref(),
            r.fallback_reason.as_deref(),
            r.failure_kind.as_deref(),
            r.local_route_kind.as_deref(),
        ]
        .into_iter()
        .enumerate()
        {
            count(&mut self.maps[index], value);
        }
        count_many(&mut self.maps[7], r.llm_call_sites.as_deref());
        count_many(&mut self.maps[8], r.context_needs.as_deref());
        count_many(&mut self.maps[9], r.tool_hints.as_deref());
        count_many(&mut self.maps[10], r.reason_codes.as_deref());
        self.route_used += yes(r.route_turn_used);
        self.local_used += yes(r.local_route_used);
        self.hints_present += yes(r.route_turn_hints_present);
        self.hints_injected += yes(r.route_turn_hints_injected);
        self.short_circuit += yes(r.short_circuit_allowed);
        self.inject_hints += yes(r.inject_hints);
        self.skipped = self
            .skipped
            .saturating_add(r.route_turn_skipped_count.unwrap_or(0));
        self.fallback = self
            .fallback
            .saturating_add(r.route_turn_fallback_count.unwrap_or(0));
        self.tokens = self
            .tokens
            .saturating_add(r.estimated_tokens_saved.unwrap_or(0));
        self.llm_calls = self
            .llm_calls
            .saturating_add(r.llm_call_count_estimated.unwrap_or(0));
        for (values, value) in self.latencies.iter_mut().zip([
            r.local_route_latency_ms,
            r.route_turn_latency_ms,
            r.agent_turn_latency_ms,
            r.total_turn_latency_ms,
        ]) {
            if let Some(value) = value.filter(|value| *value > 0) {
                values.push(value);
            }
        }
    }

    fn finish(mut self, valid: usize, total: usize, invalid: usize) -> SmartObservationStats {
        let take =
            |maps: &mut [BTreeMap<String, usize>; 11], index| std::mem::take(&mut maps[index]);
        SmartObservationStats {
            total_records: total,
            valid_records: valid,
            invalid_lines: invalid,
            first_timestamp_ms: self.first,
            last_timestamp_ms: self.last,
            by_mode: take(&mut self.maps, 0),
            by_intent: take(&mut self.maps, 1),
            by_gate: take(&mut self.maps, 2),
            by_decision_path: take(&mut self.maps, 3),
            by_fallback_reason: take(&mut self.maps, 4),
            by_failure_kind: take(&mut self.maps, 5),
            by_local_route_kind: take(&mut self.maps, 6),
            route_turn_used_count: self.route_used,
            route_turn_skipped_count: self.skipped,
            route_turn_fallback_count: self.fallback,
            local_route_used_count: self.local_used,
            route_turn_hints_present_count: self.hints_present,
            route_turn_hints_injected_count: self.hints_injected,
            short_circuit_allowed_count: self.short_circuit,
            inject_hints_count: self.inject_hints,
            estimated_tokens_saved_sum: self.tokens,
            latency: LatencyStats {
                local_route: latency(std::mem::take(&mut self.latencies[0])),
                route_turn: latency(std::mem::take(&mut self.latencies[1])),
                agent_turn: latency(std::mem::take(&mut self.latencies[2])),
                total_turn: latency(std::mem::take(&mut self.latencies[3])),
            },
            llm_call_count_estimated_sum: self.llm_calls,
            llm_call_sites: take(&mut self.maps, 7),
            context_needs: take(&mut self.maps, 8),
            tool_hints: take(&mut self.maps, 9),
            reason_codes: take(&mut self.maps, 10),
        }
    }
}

pub fn filter_observations(
    records: Vec<SmartObservationLine>,
    session: Option<&str>,
    since: Option<u64>,
) -> Vec<SmartObservationLine> {
    records
        .into_iter()
        .filter(|record| {
            session.is_none_or(|value| record.ai_session_id.as_deref() == Some(value))
                && since.is_none_or(|value| {
                    record
                        .timestamp_ms
                        .is_some_and(|timestamp| timestamp >= value)
                })
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct SmartObservationRecent {
    pub timestamp_ms: Option<u64>,
    pub mode: Option<String>,
    pub intent: Option<String>,
    pub confidence_bps: Option<u16>,
    pub gate: Option<String>,
    pub decision_path: Option<String>,
    pub route_turn_used: bool,
    pub route_turn_required: bool,
    pub short_circuit_allowed: bool,
    pub inject_hints: bool,
    pub route_turn_hints_present: bool,
    pub route_turn_hints_injected: bool,
    pub fallback_reason: Option<String>,
    pub failure_kind: Option<String>,
    pub local_route_kind: Option<String>,
    pub local_route_used: bool,
    pub route_turn_skipped_count: u64,
    pub route_turn_fallback_count: u64,
    pub local_route_latency_ms: u64,
    pub route_turn_latency_ms: u64,
    pub agent_turn_latency_ms: u64,
    pub total_turn_latency_ms: u64,
    pub estimated_tokens_saved: u64,
    pub context_needs: Vec<String>,
    pub tool_hints: Vec<String>,
    pub reason_codes: Vec<String>,
}

impl From<&SmartObservationLine> for SmartObservationRecent {
    fn from(r: &SmartObservationLine) -> Self {
        Self {
            timestamp_ms: r.timestamp_ms,
            mode: r.mode.clone(),
            intent: r.intent.clone(),
            confidence_bps: r.confidence_bps,
            gate: r.gate.clone(),
            decision_path: r.decision_path.clone(),
            route_turn_used: r.route_turn_used.unwrap_or(false),
            route_turn_required: r.route_turn_required.unwrap_or(false),
            short_circuit_allowed: r.short_circuit_allowed.unwrap_or(false),
            inject_hints: r.inject_hints.unwrap_or(false),
            route_turn_hints_present: r.route_turn_hints_present.unwrap_or(false),
            route_turn_hints_injected: r.route_turn_hints_injected.unwrap_or(false),
            fallback_reason: r.fallback_reason.clone(),
            failure_kind: r.failure_kind.clone(),
            local_route_kind: r.local_route_kind.clone(),
            local_route_used: r.local_route_used.unwrap_or(false),
            route_turn_skipped_count: r.route_turn_skipped_count.unwrap_or(0),
            route_turn_fallback_count: r.route_turn_fallback_count.unwrap_or(0),
            local_route_latency_ms: r.local_route_latency_ms.unwrap_or(0),
            route_turn_latency_ms: r.route_turn_latency_ms.unwrap_or(0),
            agent_turn_latency_ms: r.agent_turn_latency_ms.unwrap_or(0),
            total_turn_latency_ms: r.total_turn_latency_ms.unwrap_or(0),
            estimated_tokens_saved: r.estimated_tokens_saved.unwrap_or(0),
            context_needs: r.context_needs.clone().unwrap_or_default(),
            tool_hints: r.tool_hints.clone().unwrap_or_default(),
            reason_codes: r.reason_codes.clone().unwrap_or_default(),
        }
    }
}

pub fn render_recent(
    records: &[SmartObservationLine],
    format: OutputFormat,
) -> Result<String, serde_json::Error> {
    let rows = records
        .iter()
        .map(SmartObservationRecent::from)
        .collect::<Vec<_>>();
    if format == OutputFormat::Json {
        return serde_json::to_string_pretty(&rows).map(|value| value + "\n");
    }
    let values = rows
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()?;
    let columns = recent_columns();
    let mut out = String::new();
    if format == OutputFormat::Tsv {
        out.push_str(&columns.join("\t"));
        out.push('\n');
    } else {
        append_env_line(&mut out, "SMART_RECENT_COUNT", &rows.len().to_string());
    }
    for (index, value) in values.iter().enumerate() {
        for (column_index, column) in columns.iter().enumerate() {
            let scalar = json_scalar(&value[*column]);
            if format == OutputFormat::Tsv {
                if column_index > 0 {
                    out.push('\t');
                }
                out.push_str(&cell(&scalar));
            } else {
                append_env_line(
                    &mut out,
                    &format!("SMART_RECENT_{index}_{}", env_key(column)),
                    &cell(&scalar),
                );
            }
        }
        if format == OutputFormat::Tsv {
            out.push('\n');
        }
    }
    Ok(out)
}

pub struct SmartReportOptions<'a> {
    pub observation_path: &'a str,
    pub limit: usize,
    pub since_hours: Option<u64>,
    pub session_filter: Option<&'a str>,
    pub include_recent: usize,
}

pub fn render_markdown_report(
    stats: &SmartObservationStats,
    records: &[SmartObservationLine],
    options: SmartReportOptions<'_>,
) -> String {
    let mut out = format!(
        "# AISH Smart Preprocessor Observation Report\n\n## Scope\n- observation_path: {}\n- total_records: {}\n- valid_records: {}\n- invalid_lines: {}\n- first_timestamp_ms: {}\n- last_timestamp_ms: {}\n- limit: {}\n- since_hours: {}\n- session_filter: {}\n\n## Summary\n",
        md(options.observation_path), stats.total_records, stats.valid_records, stats.invalid_lines,
        stats.first_timestamp_ms.unwrap_or(0), stats.last_timestamp_ms.unwrap_or(0), options.limit,
        options.since_hours.map_or_else(|| "all".into(), |value| value.to_string()),
        md(options.session_filter.unwrap_or("all"))
    );
    dist(&mut out, "mode distribution", &stats.by_mode);
    dist(&mut out, "intent distribution", &stats.by_intent);
    dist(&mut out, "gate distribution", &stats.by_gate);
    dist(
        &mut out,
        "decision_path distribution",
        &stats.by_decision_path,
    );
    for (key, value) in [
        ("route_turn_used_count", stats.route_turn_used_count as u64),
        (
            "local_route_used_count",
            stats.local_route_used_count as u64,
        ),
        (
            "short_circuit_allowed_count",
            stats.short_circuit_allowed_count as u64,
        ),
        (
            "route_turn_hints_present_count",
            stats.route_turn_hints_present_count as u64,
        ),
        (
            "route_turn_hints_injected_count",
            stats.route_turn_hints_injected_count as u64,
        ),
        (
            "estimated_tokens_saved_sum",
            stats.estimated_tokens_saved_sum,
        ),
        (
            "llm_call_count_estimated_sum",
            stats.llm_call_count_estimated_sum,
        ),
    ] {
        out.push_str(&format!("- {key}: {value}\n"));
    }
    out.push_str("\n## Latency\n");
    for (key, value) in [
        ("local_route", &stats.latency.local_route),
        ("route_turn", &stats.latency.route_turn),
        ("agent_turn", &stats.latency.agent_turn),
        ("total_turn", &stats.latency.total_turn),
    ] {
        out.push_str(&format!(
            "- {key}: avg={:.2} ms, p50={} ms, p95={} ms, samples={}\n",
            value.avg_ms, value.p50_ms, value.p95_ms, value.sample_count
        ));
    }
    out.push_str("\n## Fallbacks\n");
    dist(
        &mut out,
        "fallback_reason distribution",
        &stats.by_fallback_reason,
    );
    dist(
        &mut out,
        "failure_kind distribution",
        &stats.by_failure_kind,
    );
    out.push_str(&format!("- route_turn_fallback_count: {}\n- likely causes: infer only from fallback_reason and failure_kind above\n\n## Context / Tool Hints\n", stats.route_turn_fallback_count));
    dist(&mut out, "context_needs distribution", &stats.context_needs);
    dist(&mut out, "tool_hints distribution", &stats.tool_hints);
    dist(&mut out, "reason_codes distribution", &stats.reason_codes);
    let start = records.len().saturating_sub(options.include_recent);
    let recent = records[start..]
        .iter()
        .map(SmartObservationRecent::from)
        .collect::<Vec<_>>();
    out.push_str("\n## Recent Observations\n\n");
    out.push_str(&serde_json::to_string_pretty(&recent).unwrap_or_else(|_| "[]".into()));
    out.push_str("\n\n## Notes for AI Evaluation\n\nThis report contains no raw user text. Classification accuracy cannot be measured directly without original user input and ground-truth labels. The following can be evaluated:\n- assist/gate usage\n- excessive short-circuiting or fallback\n- route_turn hints present but not injected\n- local_route latency and estimated token savings\n- intent / context_needs / tool_hints skew\n");
    out
}

fn count(map: &mut BTreeMap<String, usize>, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        *map.entry(value.into()).or_default() += 1;
    }
}
fn count_many(map: &mut BTreeMap<String, usize>, values: Option<&[String]>) {
    for value in values.unwrap_or_default() {
        count(map, Some(value));
    }
}
fn yes(value: Option<bool>) -> usize {
    usize::from(value == Some(true))
}
fn empty_latency() -> LatencySummary {
    LatencySummary {
        sample_count: 0,
        avg_ms: 0.0,
        p50_ms: 0,
        p95_ms: 0,
    }
}
fn latency(mut values: Vec<u64>) -> LatencySummary {
    if values.is_empty() {
        return empty_latency();
    }
    values.sort_unstable();
    let len = values.len();
    let sum: u128 = values.iter().map(|value| *value as u128).sum();
    let rank = |percentile: usize| values[(percentile * len).div_ceil(100) - 1];
    LatencySummary {
        sample_count: len,
        avg_ms: sum as f64 / len as f64,
        p50_ms: rank(50),
        p95_ms: rank(95),
    }
}
fn flatten(prefix: &str, value: &serde_json::Value, out: &mut Vec<(String, String)>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                let name = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten(&name, value, out);
            }
        }
        _ => out.push((prefix.into(), json_scalar(value))),
    }
}
fn json_scalar(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "0".into(),
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Array(values) => {
            values.iter().map(json_scalar).collect::<Vec<_>>().join(",")
        }
        _ => value.to_string(),
    }
}
fn cell(value: &str) -> String {
    value.replace(['\t', '\n', '\r'], " ")
}
fn env_key(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}
fn md(value: &str) -> String {
    cell(value).replace('|', "\\|")
}
fn dist(out: &mut String, key: &str, map: &BTreeMap<String, usize>) {
    let value = if map.is_empty() {
        "none".into()
    } else {
        map.iter()
            .map(|(key, value)| format!("{}={value}", md(key)))
            .collect::<Vec<_>>()
            .join(", ")
    };
    out.push_str(&format!("- {key}: {value}\n"));
}
fn recent_columns() -> Vec<&'static str> {
    vec![
        "timestamp_ms",
        "mode",
        "intent",
        "confidence_bps",
        "gate",
        "decision_path",
        "route_turn_used",
        "route_turn_required",
        "short_circuit_allowed",
        "inject_hints",
        "route_turn_hints_present",
        "route_turn_hints_injected",
        "fallback_reason",
        "failure_kind",
        "local_route_kind",
        "local_route_used",
        "route_turn_skipped_count",
        "route_turn_fallback_count",
        "local_route_latency_ms",
        "route_turn_latency_ms",
        "agent_turn_latency_ms",
        "total_turn_latency_ms",
        "estimated_tokens_saved",
        "context_needs",
        "tool_hints",
        "reason_codes",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(timestamp: u64) -> SmartObservationLine {
        SmartObservationLine {
            timestamp_ms: Some(timestamp),
            ai_session_id: Some("a".into()),
            mode: Some("gate".into()),
            intent: Some("debug".into()),
            gate: Some("assist".into()),
            decision_path: Some("route".into()),
            route_turn_used: Some(true),
            local_route_used: Some(true),
            estimated_tokens_saved: Some(5),
            context_needs: Some(vec!["git".into()]),
            tool_hints: Some(vec!["shell".into()]),
            reason_codes: Some(vec!["reason".into()]),
            total_turn_latency_ms: Some(timestamp),
            ..Default::default()
        }
    }
    #[test]
    fn stats_aggregates_distributions_counts_and_latency() {
        let stats =
            SmartObservationStats::from_records(&[record(10), record(20), record(100)], 4, 1);
        assert_eq!(stats.valid_records, 3);
        assert_eq!(stats.by_mode["gate"], 3);
        assert_eq!(stats.route_turn_used_count, 3);
        assert_eq!(stats.estimated_tokens_saved_sum, 15);
        assert_eq!(stats.latency.total_turn.p50_ms, 20);
        assert_eq!(stats.latency.total_turn.p95_ms, 100);
    }
    #[test]
    fn filters_session_and_since_hours() {
        let mut other = record(30);
        other.ai_session_id = Some("b".into());
        let records = filter_observations(vec![record(10), record(20), other], Some("a"), Some(15));
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].timestamp_ms, Some(20));
    }
    #[test]
    fn report_drops_unknown_raw_fields() {
        let record: SmartObservationLine =
            serde_json::from_str(r#"{"timestamp_ms":1,"raw_user_text":"secret"}"#).unwrap();
        let stats = SmartObservationStats::from_records(std::slice::from_ref(&record), 1, 0);
        let report = render_markdown_report(
            &stats,
            &[record],
            SmartReportOptions {
                observation_path: "x",
                limit: 1,
                since_hours: None,
                session_filter: None,
                include_recent: 1,
            },
        );
        assert!(!report.contains("secret"));
        assert!(!report.contains("raw_user_text"));
    }
}
