mod adapter;
mod cli;
mod domain;
mod usecase;

use std::process;
#[cfg(unix)]
use usecase::wire_aish;
use common::error::Error;
use cli::parse_args;

fn main() {
    let exit_code = match run() {
        Ok(code) => code,
        Err(e) => {
            if e.is_usage() {
                usecase::print_usage();
            }
            eprintln!("aish: {}", e);
            e.exit_code()
        }
    };
    process::exit(exit_code);
}

pub fn run() -> Result<i32, Error> {
    let config = parse_args()?;
    #[cfg(unix)]
    {
        let use_case = wire_aish();
        use_case.run(config)
    }
    #[cfg(not(unix))]
    {
        let _ = config;
        Err(Error::system("aish is only supported on Unix"))
    }
}

#[cfg(test)]
mod tests {
    // 実際の引数解析は環境変数に依存するため、
    // 統合テストで確認する
}
