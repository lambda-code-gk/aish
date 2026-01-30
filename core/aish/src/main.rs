mod args;
mod app;
mod shell;
mod terminal;
mod platform;

use std::process;
use common::error::Error;
use args::parse_args;
use app::run_app;

fn main() {
    let exit_code = match run() {
        Ok(code) => code,
        Err((msg, code)) => {
            if code == 64 {
                app::print_usage();
            }
            eprintln!("aish: {}", msg);
            code
        }
    };
    process::exit(exit_code);
}

pub fn run() -> Result<i32, Error> {
    let config = parse_args()?;
    run_app(config)
}

#[cfg(test)]
mod tests {
    // 実際の引数解析は環境変数に依存するため、
    // 統合テストで確認する
}

