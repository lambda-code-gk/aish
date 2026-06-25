//! CLI 出力形式（`--format`）。

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    #[default]
    Tsv,
    Json,
    Env,
}

impl From<OutputFormat> for aish_replay::OutputFormat {
    fn from(value: OutputFormat) -> Self {
        match value {
            OutputFormat::Tsv => Self::Tsv,
            OutputFormat::Json => Self::Json,
            OutputFormat::Env => Self::Env,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OutputFormatError {
    #[error("unknown --format {0:?} (expected tsv, json, or env)")]
    Unknown(String),
}

impl OutputFormat {
    pub fn parse(s: &str) -> Result<Self, OutputFormatError> {
        match s {
            "tsv" => Ok(Self::Tsv),
            "json" => Ok(Self::Json),
            "env" => Ok(Self::Env),
            other => Err(OutputFormatError::Unknown(other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_tsv() {
        assert_eq!(OutputFormat::default(), OutputFormat::Tsv);
    }

    #[test]
    fn parses_three_formats() {
        assert_eq!(OutputFormat::parse("tsv").unwrap(), OutputFormat::Tsv);
        assert_eq!(OutputFormat::parse("json").unwrap(), OutputFormat::Json);
        assert_eq!(OutputFormat::parse("env").unwrap(), OutputFormat::Env);
    }
}
