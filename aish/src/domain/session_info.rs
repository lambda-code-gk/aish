//! 現在セッションの表示用情報。

use serde::Serialize;

use crate::domain::OutputFormat;

/// `AISH_SESSION_DIR` から得るセッション情報。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub session_dir: String,
    pub log_file: String,
    pub current_log: String,
}

impl SessionInfo {
    pub fn render(&self, format: OutputFormat) -> String {
        match format {
            OutputFormat::Tsv => self.render_tsv(),
            OutputFormat::Json => self.render_json(),
            OutputFormat::Env => self.render_env(),
        }
    }

    fn render_tsv(&self) -> String {
        let mut out = String::new();
        append_tsv_row(&mut out, "session_id", &self.session_id);
        append_tsv_row(&mut out, "session_dir", &self.session_dir);
        append_tsv_row(&mut out, "log_file", &self.log_file);
        append_tsv_row(&mut out, "current_log", &self.current_log);
        out
    }

    fn render_json(&self) -> String {
        serde_json::to_string(self).expect("SessionInfo serializes")
    }

    fn render_env(&self) -> String {
        let mut out = String::new();
        append_env_line(&mut out, "AISH_SESSION_DIR", &self.session_dir);
        append_env_line(&mut out, "AISH_SESSION_ID", &self.session_id);
        append_env_line(&mut out, "AISH_LOG_FILE", &self.log_file);
        append_env_line(&mut out, "AISH_CURRENT_LOG", &self.current_log);
        out
    }
}

fn append_tsv_row(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push('\t');
    out.push_str(value);
    out.push('\n');
}

fn append_env_line(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push('=');
    out.push_str(&shell_single_quote(value));
    out.push('\n');
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SessionInfo {
        SessionInfo {
            session_id: "002f15d02b54".to_string(),
            session_dir: "/tmp/s/002f15d02b54".to_string(),
            log_file: "/tmp/s/002f15d02b54/log.jsonl".to_string(),
            current_log: "/tmp/s/002f15d02b54/current_log".to_string(),
        }
    }

    #[test]
    fn tsv_has_four_rows() {
        let out = sample().render(OutputFormat::Tsv);
        assert_eq!(out.matches('\n').count(), 4);
        assert!(out.contains("session_id\t002f15d02b54"));
    }

    #[test]
    fn json_roundtrip_fields() {
        let out = sample().render(OutputFormat::Json);
        let v: serde_json::Value = serde_json::from_str(&out).expect("json");
        assert_eq!(v["session_id"], "002f15d02b54");
    }

    #[test]
    fn env_quotes_special_chars() {
        let info = SessionInfo {
            session_dir: "/tmp/a b".to_string(),
            ..sample()
        };
        let out = info.render(OutputFormat::Env);
        assert!(out.contains("AISH_SESSION_DIR='/tmp/a b'"));
    }
}
