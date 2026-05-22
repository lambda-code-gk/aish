//! ログ出力のマスク。

use regex::Regex;
use std::sync::LazyLock;

static RE_SK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"sk-[a-zA-Z0-9]{8,}").expect("regex"));
static RE_BEARER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)bearer\s+[a-zA-Z0-9._\-]+").expect("regex"));
static RE_ENV_SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)([A-Za-z0-9_]*(?:KEY|TOKEN|SECRET)[A-Za-z0-9_]*)=([^\s\\]+)").expect("regex")
});
static RE_AIZA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"AIza[0-9A-Za-z_\-]{10,}").expect("regex"));

/// ログ・コンテキストへ書く前に機微らしき部分を置換する。
pub fn sanitize_log_text(input: &str) -> String {
    let mut s = input.to_string();
    s = RE_SK.replace_all(&s, "sk-[REDACTED]").into_owned();
    s = RE_BEARER.replace_all(&s, "Bearer [REDACTED]").into_owned();
    s = RE_AIZA.replace_all(&s, "AIza[REDACTED]").into_owned();
    s = RE_ENV_SECRET.replace_all(&s, "$1=[REDACTED]").into_owned();
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_api_key_patterns() {
        let raw = "key=sk-abcdefghijklmnopqrst export APP_SECRET=secret123 Bearer abc.def";
        let out = sanitize_log_text(raw);
        assert!(!out.contains("sk-abcdefghijklmnopqrst"));
        assert!(!out.contains("secret123"));
        assert!(!out.contains("Bearer abc.def"));
        assert!(out.contains("sk-[REDACTED]"));
        assert!(out.contains("APP_SECRET=[REDACTED]"));
    }
}
