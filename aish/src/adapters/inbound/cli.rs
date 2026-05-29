//! 全サブコマンド共通の CLI オプション。
//!
//! `--format` は情報表示系サブコマンド向けの出力形式。全サブコマンドで解析するが、
//! 現状 stdout に反映するのは `session` のみ（実行系は受理のみ）。詳細: `docs/architecture.md`。

use crate::domain::{OutputFormat, OutputFormatError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CommonOptions {
    pub format: OutputFormat,
}

#[derive(Debug, thiserror::Error)]
pub enum CommonOptionsError {
    #[error("--format requires a value")]
    MissingFormatValue,
    #[error(transparent)]
    Format(#[from] OutputFormatError),
}

/// `args` から `--format` を取り除き、残りをサブコマンド向けに返す。
pub fn strip_common_options(args: &mut Vec<String>) -> Result<CommonOptions, CommonOptionsError> {
    let mut format = OutputFormat::default();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--format" {
            let value = args
                .get(i + 1)
                .ok_or(CommonOptionsError::MissingFormatValue)?;
            format = OutputFormat::parse(value)?;
            args.remove(i + 1);
            args.remove(i);
            continue;
        }
        i += 1;
    }
    Ok(CommonOptions { format })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_format_and_leaves_rest() {
        let mut args = vec![
            "--format".into(),
            "json".into(),
            "--log".into(),
            "/tmp/x".into(),
        ];
        let opts = strip_common_options(&mut args).expect("parse");
        assert_eq!(opts.format, OutputFormat::Json);
        assert_eq!(args, vec!["--log", "/tmp/x"]);
    }

    #[test]
    fn default_when_absent() {
        let mut args = vec!["--".into(), "echo".into()];
        let opts = strip_common_options(&mut args).expect("parse");
        assert_eq!(opts.format, OutputFormat::Tsv);
    }
}
