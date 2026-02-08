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
use cli::{config_to_command, parse_args, print_completion, Config, ParseOutcome};
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
            AiCommand::ListProfiles => {
                let (names, default) = self.app.ai_use_case.list_profiles()?;
                for name in &names {
                    if default.as_deref() == Some(name.as_str()) {
                        println!("{} (default)", name);
                    } else {
                        println!("{}", name);
                    }
                }
                Ok(0)
            }
            AiCommand::Task {
                name,
                args,
                profile,
                model,
                system,
            } => self.app.task_use_case.run(
                session_dir,
                &name,
                &args,
                profile,
                model,
                system_instruction(system).as_deref(),
            ),
            AiCommand::Resume {
                profile,
                model,
                system,
            } => {
                let max_turns = std::env::var("AI_MAX_TURNS")
                    .ok()
                    .and_then(|s| s.parse::<usize>().ok());
                self.app.run_query.run_query(
                    session_dir,
                    profile,
                    model,
                    None,
                    system_instruction(system).as_deref(),
                    max_turns,
                )
            }
            AiCommand::Query {
                profile,
                model,
                query,
                system,
            } => {
                if query.trim().is_empty() {
                    return Err(Error::invalid_argument(
                        "No query provided. Use -c or --continue to resume a previous session.",
                    ));
                }
                let max_turns = std::env::var("AI_MAX_TURNS")
                    .ok()
                    .and_then(|s| s.parse::<usize>().ok());
                self.app.run_query.run_query(
                    session_dir,
                    profile,
                    model,
                    Some(&query),
                    system_instruction(system).as_deref(),
                    max_turns,
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
        AiCommand::ListProfiles => "list-profiles",
        AiCommand::Task { .. } => "task",
        AiCommand::Resume { .. } => "resume",
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
    let outcome = parse_args()?;
    let config = match &outcome {
        ParseOutcome::Config(c) => c.clone(),
        ParseOutcome::GenerateCompletion(shell) => {
            print_completion(*shell);
            return Ok(0);
        }
        ParseOutcome::ListTasks => {
            let app = wire_ai(false);
            let names = app.task_use_case.list_names()?;
            for n in &names {
                println!("{}", n);
            }
            return Ok(0);
        }
    };
    let app = wire_ai(config.non_interactive);
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
    println!("  -L, --list-profiles           List currently available provider profiles (from profiles.json + built-ins)");
    println!("  -c, --continue                Resume the agent loop from the last saved state (after turn limit or error). Uses AISH_SESSION when set.");
    println!("  --no-interactive              Do not prompt for confirmations (CI-friendly: tool approval denied, no continue, leakscan deny).");
    println!("  -p, --profile <profile>         Specify LLM profile (gemini, gpt, echo, etc.). Default: profiles.json default, or gemini if not set.");
    println!("  -m, --model <model>            Specify model name (e.g. gemini-2.0, gpt-4). Default: profile default from profiles.json");
    println!("  -S, --system <instruction>     Set system instruction (e.g. role or constraints) for this query");
    println!("                                If omitted, enabled system prompts from aish sysq are used.");
    println!("  --generate <shell>             Generate shell completion script (bash, zsh, fish). Source the output to enable tab completion.");
    println!("  --list-tasks                   List available task names (used by shell completion).");
    println!();
    println!("Environment:");
    println!("  AISH_SESSION    Session directory for resume/continue. Set by aish when running ai from the shell.");
    println!("  AISH_HOME       Home directory. Profiles: $AISH_HOME/config/profiles.json; tasks: $AISH_HOME/config/task.d/");
    println!("                 If unset, $XDG_CONFIG_HOME/aish (e.g. ~/.config/aish) is used.");
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
    println!("  ai --profile echo Explain quantum computing");
    println!("  ai mytask do something");
}
