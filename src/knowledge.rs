use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Pattern {
    pub id: String,
    pub pattern: String,
    pub description: String,
    pub suggestion: Suggestion,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Suggestion {
    pub message: String,
    pub action_type: String,
    pub command_hint: Option<String>,
    pub script_path: Option<String>,
}

pub fn load_troubleshooting_patterns() -> Vec<Pattern> {
    let mut patterns = Vec::new();

    // Attempt to locate the knowledge file relative to the executable or project root
    let paths = [
        "knowledge/troubleshooting.json",
        "../knowledge/troubleshooting.json",
        "../../knowledge/troubleshooting.json",
    ];

    for path in paths {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(loaded) = serde_json::from_str::<Vec<Pattern>>(&content) {
                patterns = loaded;
                break;
            }
        }
    }

    patterns
}

pub fn match_error_pattern(stderr: &str, patterns: &[Pattern]) -> Option<Suggestion> {
    for p in patterns {
        if let Ok(re) = regex::Regex::new(&p.pattern) {
            if re.is_match(stderr) {
                return Some(p.suggestion.clone());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_windows_acl_error_matching() {
        let patterns = vec![Pattern {
            id: "WIN_SSH_ACL_DENIED".to_string(),
            pattern: "Permission denied .*publickey.*".to_string(),
            description: "Windows ACL Error".to_string(),
            suggestion: Suggestion {
                message: "ACL Error detected".to_string(),
                action_type: "run_script".to_string(),
                command_hint: None,
                script_path: Some("scripts/fix_acl.py".to_string()),
            },
        }];

        let stderr_input = "user@host: Permission denied (publickey,keyboard-interactive).";
        let suggestion = match_error_pattern(stderr_input, &patterns);

        assert!(suggestion.is_some());
        assert_eq!(suggestion.unwrap().message, "ACL Error detected");
    }
}
