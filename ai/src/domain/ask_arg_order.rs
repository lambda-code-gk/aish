//! `ai ask` の引数順検証（オプションはメッセージより前）。

use thiserror::Error;

const VALUE_OPTIONS: &[&str] = &[
    "--log",
    "--log-tail",
    "--preset",
    "--session",
    "--socket",
    "--tools",
    "--profile",
    "--file",
    "--format",
    "--limit",
];
const FLAG_OPTIONS: &[&str] = &[
    "--quiet",
    "--dry-run",
    "--no-log",
    "--no-start",
    "--verbose-tools",
];

#[derive(Debug, Error, PartialEq, Eq)]
#[error("options must appear before message")]
pub struct AskArgOrderError;

/// `ask` サブコマンド直後の引数列を検証する。
pub fn validate_ask_arg_order(args: &[String]) -> Result<(), AskArgOrderError> {
    let Some(msg_idx) = first_message_arg_index(args) else {
        return Ok(());
    };
    for arg in args.iter().skip(msg_idx + 1) {
        if is_option_token(arg) {
            return Err(AskArgOrderError);
        }
    }
    Ok(())
}

fn first_message_arg_index(args: &[String]) -> Option<usize> {
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--" {
            return Some(i + 1);
        }
        if arg == "-" {
            return Some(i);
        }
        if arg == "-f" {
            i += 2;
            continue;
        }
        if arg == "-q" {
            i += 1;
            continue;
        }
        if arg.strip_prefix("--").is_some() {
            if FLAG_OPTIONS.contains(&arg) {
                i += 1;
                continue;
            }
            if VALUE_OPTIONS.contains(&arg) {
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if arg.starts_with('-') {
            i += 1;
            continue;
        }
        return Some(i);
    }
    None
}

fn is_option_token(arg: &str) -> bool {
    arg.starts_with('-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_options_before_message() {
        validate_ask_arg_order(&["--log".into(), "/tmp/x".into(), "hello".into()]).expect("valid");
    }

    #[test]
    fn rejects_options_after_message() {
        let err =
            validate_ask_arg_order(&["hello".into(), "--log".into(), "/tmp/x".into()]).unwrap_err();
        assert_eq!(err, AskArgOrderError);
    }
}
