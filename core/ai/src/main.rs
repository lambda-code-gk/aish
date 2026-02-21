mod adapter;
mod cli;
mod domain;
mod ports;
mod usecase;
mod wiring;

#[cfg(test)]
mod tests;

use std::process;
use common::domain::{ModelName, ProviderName, SessionDir};
use common::error::Error;
use common::ports::outbound::{now_iso8601, Log, LogLevel, LogRecord};
use cli::{config_to_command, parse_args, print_completion, Config, ParseOutcome};
use domain::{AiCommand, TaskName};
use ports::inbound::UseCaseRunner;
use common::event_hub::{build_event_hub, EventHubHandle};
use wiring::{wire_ai, App};

/// Command をディスパッチする Runner（match は main レイヤーに集約）
struct Runner {
    app: App,
}

impl Runner {
    /// モード解決と -S 未指定時のフック解決で config を補完する。
    fn prepare_config(&self, config: &mut Config) -> Result<(), Error> {
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
        if config.system.is_none() {
            if let Some(s) = self.app.resolve_system_prompt_from_hooks.resolve_system_prompt_from_hooks()? {
                config.system = Some(s);
            }
        }
        Ok(())
    }

    fn run_dry_run(
        &self,
        cmd: AiCommand,
        session_dir: Option<SessionDir>,
        mode_name_for_dry_run: Option<String>,
    ) -> Result<i32, Error> {
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
        self.app.ai_use_case.run_dry_run(
            session_dir,
            profile,
            model,
            query_opt.as_ref(),
            system_opt.as_deref(),
            tool_allowlist,
            mode_name,
        )?;
        Ok(0)
    }

    fn run_list_profiles(&self) -> Result<i32, Error> {
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

    fn run_list_tools(&self, profile: Option<common::domain::ProviderName>) -> Result<i32, Error> {
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

    fn run_task(
        &self,
        session_dir: Option<SessionDir>,
        name: &TaskName,
        args: &[String],
        profile: Option<ProviderName>,
        model: Option<ModelName>,
        system: Option<String>,
        tool_allowlist: Option<Vec<String>>,
        event_hub: EventHubHandle,
    ) -> Result<i32, Error> {
        self.app.task_use_case.run(
            session_dir,
            name,
            args,
            profile,
            model,
            system.as_deref(),
            tool_allowlist.as_deref(),
            Some(event_hub),
        )
    }

    fn run_resume(
        &self,
        session_dir: Option<SessionDir>,
        profile: Option<ProviderName>,
        model: Option<ModelName>,
        system: Option<String>,
        tool_allowlist: Option<Vec<String>>,
        event_hub: EventHubHandle,
    ) -> Result<i32, Error> {
        let max_turns = std::env::var("AI_MAX_TURNS")
            .ok()
            .and_then(|s| s.parse::<usize>().ok());
        self.app.run_query.run_query(
            session_dir,
            profile,
            model,
            None,
            system.as_deref(),
            max_turns,
            tool_allowlist.as_deref(),
            Some(event_hub),
        )
    }

    fn run_query_cmd(
        &self,
        session_dir: Option<SessionDir>,
        profile: Option<ProviderName>,
        model: Option<ModelName>,
        query: &crate::domain::Query,
        system: Option<String>,
        tool_allowlist: Option<Vec<String>>,
        event_hub: EventHubHandle,
    ) -> Result<i32, Error> {
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
            Some(query),
            system.as_deref(),
            max_turns,
            tool_allowlist.as_deref(),
            Some(event_hub),
        )
    }
}

impl UseCaseRunner for Runner {
    fn run(&self, mut config: Config) -> Result<i32, Error> {
        self.prepare_config(&mut config)?;
        let session_dir = self.app.env_resolver.session_dir_from_env();
        let verbose = config.verbose;
        let dry_run = config.dry_run;
        let mode_name_for_dry_run = config.mode.clone();
        let profile_for_log = config.profile.clone();
        let model_for_log = config.model.clone();
        let task_for_log = config.task.clone();
        let message_args_len = config.message_args.len();
        let non_interactive = config.non_interactive;
        let cmd = config_to_command(config);
        let command_name = cmd_name_for_log(&cmd);

        log_command_start(self.app.logger.as_ref(), command_name);
        if verbose {
            log_verbose_snapshot(
                self.app.logger.as_ref(),
                command_name,
                non_interactive,
                profile_for_log.as_ref().map(|p| p.as_ref()),
                model_for_log.as_ref().map(|m| m.as_ref()),
                task_for_log.as_ref().map(|t| t.as_ref()),
                message_args_len,
            );
        }

        let event_hub = build_event_hub(
            session_dir.as_ref(),
            self.app.env_resolver.clone(),
            self.app.fs.clone(),
            verbose,
        );

        if dry_run {
            return self.run_dry_run(cmd, session_dir, mode_name_for_dry_run);
        }

        let result = match cmd {
            AiCommand::Help => {
                print_help();
                Ok(0)
            }
            AiCommand::ListProfiles => self.run_list_profiles(),
            AiCommand::ListTools { profile } => self.run_list_tools(profile),
            AiCommand::Task {
                name,
                args,
                profile,
                model,
                system,
                tool_allowlist,
            } => self.run_task(
                session_dir,
                &name,
                &args,
                profile,
                model,
                system,
                tool_allowlist,
                event_hub,
            ),
            AiCommand::Resume {
                profile,
                model,
                system,
                tool_allowlist,
            } => self.run_resume(
                session_dir,
                profile,
                model,
                system,
                tool_allowlist,
                event_hub.clone(),
            ),
            AiCommand::Query {
                profile,
                model,
                query,
                system,
                tool_allowlist,
            } => self.run_query_cmd(
                session_dir,
                profile,
                model,
                &query,
                system,
                tool_allowlist,
                event_hub,
            ),
        };

        log_command_finish(self.app.logger.as_ref(), command_name, &result);
        result
    }
}

fn log_command_start(logger: &dyn Log, command_name: &str) {
    let _ = logger.log(&LogRecord {
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
}

fn log_verbose_snapshot(
    logger: &dyn Log,
    command_name: &str,
    non_interactive: bool,
    profile: Option<&str>,
    model: Option<&str>,
    task: Option<&str>,
    message_args_len: usize,
) {
    let mut m = std::collections::BTreeMap::new();
    m.insert("command".to_string(), serde_json::json!(command_name));
    m.insert("non_interactive".to_string(), serde_json::json!(non_interactive));
    if let Some(p) = profile {
        m.insert("profile".to_string(), serde_json::json!(p));
    }
    if let Some(mname) = model {
        m.insert("model".to_string(), serde_json::json!(mname));
    }
    if let Some(t) = task {
        m.insert("task".to_string(), serde_json::json!(t));
    }
    if message_args_len > 0 {
        m.insert("message_args_len".to_string(), serde_json::json!(message_args_len));
    }
    let _ = logger.log(&LogRecord {
        ts: now_iso8601(),
        level: LogLevel::Debug,
        message: "verbose: config snapshot".to_string(),
        layer: Some("cli".to_string()),
        kind: Some("debug".to_string()),
        fields: Some(m),
    });
}

fn log_command_finish(logger: &dyn Log, command_name: &str, result: &Result<i32, Error>) {
    let code = result.as_ref().copied().unwrap_or(0);
    let _ = logger.log(&LogRecord {
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
        let _ = logger.log(&LogRecord {
            ts: now_iso8601(),
            level: LogLevel::Error,
            message: e.to_string(),
            layer: Some("cli".to_string()),
            kind: Some("error".to_string()),
            fields: None,
        });
    }
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
