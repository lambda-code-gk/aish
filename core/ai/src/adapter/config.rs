//! 許可コマンド設定の読み込み（adapter 層）

use common::error::Error;
use common::domain::HomeDir;
use common::tool::CommandAllowRule;
use regex::Regex;
use std::fs;
use std::path::Path;

/// 許可コマンドのルールリストを AISH_HOME/config/command_rules.txt から読み込む
pub fn load_command_allow_rules(home_dir: &HomeDir) -> Vec<CommandAllowRule> {
    let config_path = home_dir.as_ref().join("config/command_rules.txt");
    if !config_path.exists() {
        return Vec::new();
    }

    match read_rules_from_file(&config_path) {
        Ok(rules) => rules,
        Err(e) => {
            eprintln!("Warning: Failed to load command_rules.txt: {}", e);
            Vec::new()
        }
    }
}

fn read_rules_from_file(path: &Path) -> Result<Vec<CommandAllowRule>, Error> {
    let content = fs::read_to_string(path).map_err(Error::from)?;
    let mut rules = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // /.../ で囲まれている場合は正規表現、それ以外は前方一致
        if line.starts_with('/') && line.ends_with('/') && line.len() > 2 {
            let pattern = &line[1..line.len() - 1];
            match Regex::new(pattern) {
                Ok(re) => rules.push(CommandAllowRule::Regex(re)),
                Err(e) => {
                    eprintln!("Warning: Invalid regex in command_rules.txt: '{}' ({})", pattern, e);
                }
            }
        } else {
            rules.push(CommandAllowRule::Prefix(line.to_string()));
        }
    }

    Ok(rules)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_read_rules_from_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("command_rules.txt");
        let mut file = fs::File::create(&file_path).unwrap();
        writeln!(file, "ls").unwrap();
        writeln!(file, "# comment").unwrap();
        writeln!(file, "").unwrap();
        writeln!(file, "/^echo .*/").unwrap();
        writeln!(file, "cat /var/log/").unwrap();

        let rules = read_rules_from_file(&file_path).unwrap();
        assert_eq!(rules.len(), 3);
        
        // ls (Prefix)
        match &rules[0] {
            CommandAllowRule::Prefix(p) => assert_eq!(p, "ls"),
            _ => panic!("Expected Prefix"),
        }
        
        // /^echo .*/ (Regex)
        match &rules[1] {
            CommandAllowRule::Regex(re) => assert!(re.is_match("echo hello")),
            _ => panic!("Expected Regex"),
        }

        // cat /var/log/ (Prefix)
        match &rules[2] {
            CommandAllowRule::Prefix(p) => assert_eq!(p, "cat /var/log/"),
            _ => panic!("Expected Prefix"),
        }
    }
}
