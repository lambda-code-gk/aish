//! memory recipe apply 前確認（shell_exec とは別扱い）。

use std::io::{IsTerminal, Write};

pub fn stdin_ready_for_memory_recipe_apply() -> bool {
    std::io::stdin().is_terminal()
}

pub fn parse_memory_recipe_apply_choice(line: &str) -> bool {
    matches!(line.trim(), "y" | "Y" | "yes" | "Yes" | "YES")
}

/// 提案適用前に yes/no を求める。非対話 stdin では false。
pub fn prompt_memory_recipe_apply() -> bool {
    if !stdin_ready_for_memory_recipe_apply() {
        eprintln!("ai: memory recipe apply denied (non-interactive stdin)");
        return false;
    }
    eprint!("Apply proposed memory operations? [y/N] ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    let Ok(n) = std::io::stdin().read_line(&mut line) else {
        eprintln!("ai: memory recipe apply denied (stdin unavailable)");
        return false;
    };
    if n == 0 {
        eprintln!("ai: memory recipe apply denied (non-interactive stdin)");
        return false;
    }
    parse_memory_recipe_apply_choice(&line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_apply_choice() {
        assert!(parse_memory_recipe_apply_choice("y\n"));
        assert!(parse_memory_recipe_apply_choice("YES"));
        assert!(!parse_memory_recipe_apply_choice("n"));
        assert!(!parse_memory_recipe_apply_choice(""));
    }
}
