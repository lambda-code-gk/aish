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
use cli::parse_args;
use ports::inbound::RunAiApp;
use wiring::wire_ai;

fn main() {
    let exit_code = match run() {
        Ok(code) => code,
        Err(e) => {
            if e.is_usage() {
                usecase::print_usage();
            }
            eprintln!("ai: {}", e);
            e.exit_code()
        }
    };
    process::exit(exit_code);
}

pub fn run() -> Result<i32, Error> {
    let config = parse_args()?;
    let use_case = wire_ai();
    use_case.run(config)
}
