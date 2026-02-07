mod adapter;
mod cli;
mod domain;
mod ports;
mod usecase;
mod wiring;

#[cfg(test)]
mod tests;

use std::process;
use common::error::Error;
use common::ports::outbound::{now_iso8601, LogLevel, LogRecord};
use cli::{config_to_command, parse_args, Config};
use domain::AiCommand;
use ports::inbound::UseCaseRunner;
use wiring::{wire_ai, App};

/// Command をディスパッチする Runner（match は main レイヤーに集約）
struct Runner {
    app: App,
}

impl UseCaseRunner for Runner {
    fn run(&self, config: Config) -> Result<i32, Error> {
        let session_dir = self.app.env_resolver.session_dir_from_env();
        let cmd = config_to_command(config);
        let command_name = cmd_name_for_log(&cmd);
        let _ = self.app.logger.log(&LogRecord {
            ts: now_iso8601(),
            level: LogLevel::Info,
            message: "command started".to_string(),
            layer: Some("cli".to_string()),
            kind: Some("lifecycle".to_string()),
            fields: {
                let mut m = std::collections::BTreeMap::new();
                m.insert("command".to_string(), serde_json::json!(command_name));
                Some(m)
            },
        });

        // -S 未指定時は有効な sysq を結合して system instruction にする
        let system_instruction = |explicit: Option<String>| {
            explicit.or_else(|| self.app.resolve_system_instruction.resolve().ok().flatten())
        };

        let result = match cmd {
            AiCommand::Help => {
                print_help();
                Ok(0)
            }
            AiCommand::Task {
                name,
                args,
                provider,
                model,
                system,
            } => self.app.task_use_case.run(
                session_dir,
                &name,
                &args,
                provider,
                model,
                system_instruction(system).as_deref(),
            ),
            AiCommand::Query {
                provider,
                model,
                query,
                system,
            } => {
                let query_opt = if query.trim().is_empty() {
                    None
                } else {
                    Some(&query)
                };
                self.app.run_query.run_query(
                    session_dir,
                    provider,
                    model,
                    query_opt,
                    system_instruction(system).as_deref(),
                )
            }
        };
        let code = result.as_ref().copied().unwrap_or(0);
        let _ = self.app.logger.log(&LogRecord {
            ts: now_iso8601(),
            level: LogLevel::Info,
            message: "command finished".to_string(),
            layer: Some("cli".to_string()),
            kind: Some("lifecycle".to_string()),
            fields: {
                let mut m = std::collections::BTreeMap::new();
                m.insert("command".to_string(), serde_json::json!(command_name));
                m.insert("exit_code".to_string(), serde_json::json!(code));
                Some(m)
            },
        });
        if let Err(ref e) = result {
            let _ = self.app.logger.log(&LogRecord {
                ts: now_iso8601(),
                level: LogLevel::Error,
                message: e.to_string(),
                layer: Some("cli".to_string()),
                kind: Some("error".to_string()),
                fields: None,
            });
        }
        result
    }
}

fn cmd_name_for_log(cmd: &AiCommand) -> &'static str {
    match cmd {
        AiCommand::Help => "help",
        AiCommand::Task { .. } => "task",
        AiCommand::Query { .. } => "query",
    }
}

fn main() {
    let exit_code = match run() {
        Ok(code) => code,
        Err(e) => {
            if e.is_usage() {
                print_usage();
            }
            eprintln!("ai: {}", e);
            e.exit_code()
        }
    };
    process::exit(exit_code);
}

pub fn run() -> Result<i32, Error> {
    let config = parse_args()?;
    let app = wire_ai();
    let runner = Runner { app };
    runner.run(config)
}

fn print_usage() {
    eprintln!("Usage: ai [options] [task] [message...]");
}

fn print_help() {
    println!("Usage: ai [options] [task] [message...]");
    println!("Options:");
    println!("  -h, --help                    Show this help message");
    println!("  -p, --provider <provider>      Specify LLM provider (gemini, gpt, echo). Default: gemini");
    println!("  -m, --model <model>            Specify model name (e.g. gemini-2.0, gpt-4). Default: provider default");
    println!("  -S, --system <instruction>     Set system instruction (e.g. role or constraints) for this query");
    println!("                                If omitted, enabled system prompts from aish sysq are used.");
    println!();
    println!("Description:");
    println!("  Send a message to the LLM and display the response.");
    println!("  If a matching task script exists, execute it instead of sending a query.");
    println!();
    println!("Task search paths:");
    println!("  $AISH_HOME/config/task.d/");
    println!("  $XDG_CONFIG_HOME/aish/task.d");
    println!();
    println!("Examples:");
    println!("  ai Hello, how are you?");
    println!("  ai -p gpt What is Rust programming language?");
    println!("  ai --provider echo Explain quantum computing");
    println!("  ai mytask do something");
}
