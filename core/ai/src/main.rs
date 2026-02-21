mod adapter;
mod cli;
mod domain;
mod ports;
mod usecase;
mod wiring;

#[cfg(test)]
mod tests;

use std::process;
use common::domain::{ModelName, ProviderName};
use common::error::Error;
use common::ports::outbound::{now_iso8601, LogLevel, LogRecord};
use cli::{config_to_command, parse_args, print_completion, Config, ParseOutcome};
use domain::{AiCommand, DryRunInfo};
use ports::inbound::UseCaseRunner;
use common::event_hub::build_event_hub;
use wiring::{wire_ai, App};

/// Command をディスパッチする Runner（match は main レイヤーに集約）
struct Runner {
    app: App,
}

impl UseCaseRunner for Runner {
    fn run(&self, mut config: Config) -> Result<i32, Error> {
        if let Some(ref mode_name) = config.mode {
            let mode_cfg = self.app.resolve_mode_config.resolve(mode_name)?;
            if let Some(mc) = mode_cfg {
                if config.system.is_none() {
                    config.system = mc.system;
                }
                if config.profile.is_none() {
                    config.profile = mc.profile.map(ProviderName::new);
                }
                if config.model.is_none() {
                    config.model = mc.model.map(ModelName::new);
                }
                if config.tool_allowlist.is_none() {
                    config.tool_allowlist = mc.tools;
                }
            }
        }
        // -S 未指定かつモードでも未設定のとき、フックからシステムプロンプトを解決
        if config.system.is_none() {
            if let Some(s) = self.app.resolve_system_prompt_from_hooks.resolve_system_prompt_from_hooks()? {
                config.system = Some(s);
            }
        }
        let session_dir = self.app.env_resolver.session_dir_from_env();
        let verbose = config.verbose;
        let non_interactive = config.non_interactive;
        let profile_for_log = config.profile.as_ref().map(|p| p.as_ref().to_string());
        let model_for_log = config.model.as_ref().map(|m| m.as_ref().to_string());
        let task_for_log = config.task.as_ref().map(|t| t.as_ref().to_string());
        let message_args_len = config.message_args.len();
        let dry_run = config.dry_run;
        let mode_name_for_dry_run = config.mode.clone();
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
        if verbose {
            let _ = self.app.logger.log(&LogRecord {
                ts: now_iso8601(),
                level: LogLevel::Debug,
                message: "verbose: config snapshot".to_string(),
                layer: Some("cli".to_string()),
                kind: Some("debug".to_string()),
                fields: {
                    let mut m = std::collections::BTreeMap::new();
                    m.insert("command".to_string(), serde_json::json!(command_name));
                    m.insert("non_interactive".to_string(), serde_json::json!(non_interactive));
                    if let Some(ref p) = profile_for_log {
                        m.insert("profile".to_string(), serde_json::json!(p));
                    }
                    if let Some(ref mname) = model_for_log {
                        m.insert("model".to_string(), serde_json::json!(mname));
                    }
                    if let Some(ref t) = task_for_log {
                        m.insert("task".to_string(), serde_json::json!(t));
                    }
                    if message_args_len > 0 {
                        m.insert("message_args_len".to_string(), serde_json::json!(message_args_len));
                    }
                    Some(m)
                },
            });
        }

        // システムプロンプトは -S / モード / フックのいずれかで設定された config.system を渡す
        let system_instruction = |explicit: Option<String>| explicit;

        let event_hub = build_event_hub(
            session_dir.as_ref(),
            self.app.env_resolver.clone(),
            self.app.fs.clone(),
            verbose,
        );

        if dry_run {
            let (profile, model, query_opt, system_opt, tool_allowlist, mode_name) = match &cmd {
                AiCommand::Task {
                    name,
                    args,
                    profile,
                    model,
                    system,
                    tool_allowlist,
                } => {
                    let q = crate::domain::Query::new(
                        [name.as_ref().to_string()]
                            .into_iter()
                            .chain(args.iter().cloned())
                            .collect::<Vec<_>>()
                            .join(" "),
                    );
                    (
                        profile.clone(),
                        model.clone(),
                        Some(q),
                        system.clone(),
                        tool_allowlist.as_deref(),
                        mode_name_for_dry_run.clone(),
                    )
                }
                AiCommand::Resume {
                    profile,
                    model,
                    system,
                    tool_allowlist,
                } => (
                    profile.clone(),
                    model.clone(),
                    None,
                    system.clone(),
                    tool_allowlist.as_deref(),
                    mode_name_for_dry_run.clone(),
                ),
                AiCommand::Query {
                    profile,
                    model,
                    query,
                    system,
                    tool_allowlist,
                } => (
                    profile.clone(),
                    model.clone(),
                    Some(query.clone()),
                    system.clone(),
                    tool_allowlist.as_deref(),
                    mode_name_for_dry_run.clone(),
                ),
                _ => return Ok(0),
            };
            if let Some(ref q) = query_opt {
                if q.trim().is_empty() {
                    return Err(Error::invalid_argument(
                        "No query provided. Use -c or --continue to resume, or provide a message.",
                    ));
                }
            }
            let info = self.app.ai_use_case.dry_run_query(
                session_dir,
                profile,
                model,
                query_opt.as_ref(),
                system_opt.as_deref(),
                tool_allowlist,
                mode_name,
            )?;
            print_dry_run(&info);
            return Ok(0);
        }

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
            AiCommand::ListTools { profile } => {
                const DESC_MAX_LEN: usize = 52;
                let tools = self.app.ai_use_case.list_tools();
                if let Some(ref p) = profile {
                    println!("Tools enabled for profile '{}':", p.as_ref());
                } else {
                    println!("Tools:");
                }
                for (name, desc) in &tools {
                    if desc.is_empty() {
                        println!("  {}", name);
                    } else {
                        let short: String = if desc.chars().count() <= DESC_MAX_LEN {
                            desc.clone()
                        } else {
                            format!("{}...", desc.chars().take(DESC_MAX_LEN).collect::<String>())
                        };
                        println!("  {}  {}", name, short);
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
                tool_allowlist,
            } => self.app.task_use_case.run(
                session_dir,
                &name,
                &args,
                profile,
                model,
                system_instruction(system).as_deref(),
                tool_allowlist.as_deref(),
                Some(event_hub.clone()),
            ),
            AiCommand::Resume {
                profile,
                model,
                system,
                tool_allowlist,
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
                    tool_allowlist.as_deref(),
                    Some(event_hub.clone()),
                )
            }
            AiCommand::Query {
                profile,
                model,
                query,
                system,
                tool_allowlist,
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
                    tool_allowlist.as_deref(),
                    Some(event_hub.clone()),
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

fn print_dry_run(info: &DryRunInfo) {
    println!("=== ai dry run ===");
    println!("profile: {}", info.profile_name);
    println!("model: {}", info.model_name);
    if let Some(ref s) = info.system_instruction {
        println!("system_instruction: |");
        for line in s.lines() {
            println!("  {}", line);
        }
    } else {
        println!("system_instruction: (none)");
    }
    match &info.mode_name {
        Some(m) => println!("mode: {}", m),
        None => println!("mode: (none)"),
    }
    match &info.tool_allowlist {
        Some(list) => println!("tool_allowlist: [{}]", list.join(", ")),
        None => println!("tool_allowlist: (all)"),
    }
    println!("tools_enabled: [{}]", info.tools_enabled.join(", "));
    println!("--- messages ({} total) ---", info.messages.len());
    for (i, m) in info.messages.iter().enumerate() {
        let (role, content) = match m {
            common::msg::Msg::System(s) => ("system", s.as_str()),
            common::msg::Msg::User(s) => ("user", s.as_str()),
            common::msg::Msg::Assistant(s) => ("assistant", s.as_str()),
            common::msg::Msg::ToolCall { call_id, name, args, .. } => {
                let args_str = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
                println!("  [{}] tool_call id={} name={} args={}", i, call_id, name, args_str);
                continue;
            }
            common::msg::Msg::ToolResult { call_id, name, result } => {
                let res_str = serde_json::to_string(result).unwrap_or_else(|_| "{}".to_string());
                println!("  [{}] tool_result id={} name={} result={}", i, call_id, name, res_str);
                continue;
            }
        };
        println!("  [{}] {}:", i, role);
        for line in content.lines() {
            println!("    {}", line);
        }
    }
    println!("=== end dry run ===");
}

fn cmd_name_for_log(cmd: &AiCommand) -> &'static str {
    match cmd {
        AiCommand::Help => "help",
        AiCommand::ListProfiles => "list-profiles",
        AiCommand::ListTools { .. } => "list-tools",
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
            let app = wire_ai(false, false);
            let names = app.task_use_case.list_names()?;
            for n in &names {
                println!("{}", n);
            }
            return Ok(0);
        }
        ParseOutcome::ListModes => {
            let app = wire_ai(false, false);
            let names = app.resolve_mode_config.list_names()?;
            for n in &names {
                println!("{}", n);
            }
            return Ok(0);
        }
    };
    let app = wire_ai(config.non_interactive, config.verbose);
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
    println!("  --list-tools                  List tools enabled for the given profile (use with -p, e.g. -p echo)");
    println!("  -c, --continue                Resume the agent loop from the last saved state (after turn limit or error). Uses AISH_SESSION when set.");
    println!("  --no-interactive              Do not prompt for confirmations (CI-friendly: tool approval denied, no continue, leakscan deny).");
    println!("  -v, --verbose                 Emit verbose debug logs to stderr (for troubleshooting).");
    println!("  --dry-run                     Show profile, model, system prompt and messages that would be sent (no LLM call).");
    println!("  -p, --profile <profile>         Specify LLM profile (gemini, gpt, echo, etc.). Default: profiles.json default, or gemini if not set.");
    println!("  -m, --model <model>            Specify model name (e.g. gemini-2.0, gpt-4). Default: profile default from profiles.json");
    println!("  -S, --system <instruction>     Set system instruction (e.g. role or constraints) for this query");
    println!("  -M, --mode <name>             Use preset (system, profile, tools from $AISH_HOME/config/mode.d/<name>.json). CLI -p/-m/-S override mode.");
    println!("  --generate <shell>             Generate shell completion script (bash, zsh, fish). Source the output to enable tab completion.");
    println!("  --list-tasks                   List available task names (used by shell completion).");
    println!("  --list-modes                   List available mode names (used by shell completion).");
    println!();
    println!("Environment:");
    println!("  AISH_SESSION    Session directory for resume/continue. Set by aish when running ai from the shell.");
    println!("  AISH_HOME       Home directory. Profiles: $AISH_HOME/config/profiles.json; tasks: $AISH_HOME/config/task.d/; modes: $AISH_HOME/config/mode.d/");
    println!("                 If unset, $XDG_CONFIG_HOME/aish (e.g. ~/.config/aish) is used.");
    println!();
    println!("Description:");
    println!("  Send a message to the LLM and display the response.");
    println!("  If a matching task script exists, execute it instead of sending a query.");
    println!();
    println!("Task search paths (first existing wins):");
    println!("  $AISH_HOME/config/task.d/");
    println!("  $XDG_CONFIG_HOME/aish/task.d/");
    println!("  ~/.config/aish/task.d/");
    println!();
    println!("Examples:");
    println!("  ai Hello, how are you?");
    println!("  ai -p gpt What is Rust programming language?");
    println!("  ai --profile echo Explain quantum computing");
    println!("  ai mytask do something");
}
