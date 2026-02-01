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

        let (is_negative, pattern) = if line.starts_with('!') {
            (true, line[1..].trim())
        } else {
            (false, line)
        };

        // /.../ で囲まれている場合は正規表現、それ以外は前方一致
        if pattern.starts_with('/') && pattern.ends_with('/') && pattern.len() > 2 {
            let regex_str = &pattern[1..pattern.len() - 1];
            match Regex::new(regex_str) {
                Ok(re) => {
                    if is_negative {
                        rules.push(CommandAllowRule::NotRegex(re));
                    } else {
                        rules.push(CommandAllowRule::Regex(re));
                    }
                }
                Err(e) => {
                    eprintln!("Warning: Invalid regex in command_rules.txt: '{}' ({})", regex_str, e);
                }
            }
        } else {
            if is_negative {
                rules.push(CommandAllowRule::NotPrefix(pattern.to_string()));
            } else {
                rules.push(CommandAllowRule::Prefix(pattern.to_string()));
            }
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
        writeln!(file, "!/sed .*-i /").unwrap();
        writeln!(file, "!rm -rf").unwrap();

        let rules = read_rules_from_file(&file_path).unwrap();
        assert_eq!(rules.len(), 5);
        
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

        // !/sed .*-i / (NotRegex)
        match &rules[3] {
            CommandAllowRule::NotRegex(re) => assert!(re.is_match("sed -i 's/a/b/'")),
            _ => panic!("Expected NotRegex"),
        }

        // !rm -rf (NotPrefix)
        match &rules[4] {
            CommandAllowRule::NotPrefix(p) => assert_eq!(p, "rm -rf"),
            _ => panic!("Expected NotPrefix"),
        }
    }
}
