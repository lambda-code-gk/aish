use std::process;

fn main() {
    let exit_code = match run() {
        Ok(code) => code,
        Err((msg, code)) => {
            eprintln!("ai: {}", msg);
            code
        }
    };
    process::exit(exit_code);
}

pub fn run() -> Result<i32, (String, i32)> {
    // TODO: 実装を追加
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_returns_success() {
        let result = run();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }
}

