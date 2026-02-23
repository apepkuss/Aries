//! Dotenv file support for per-skill environment variables.
//!
//! Each skill can have a `.env` file in its directory (`~/.moss/skills/<name>/.env`)
//! containing user-configured environment variables (e.g., API keys).
//!
//! File format: standard `KEY=VALUE` per line, `#` for comments, empty lines ignored.

use std::{collections::HashMap, path::Path};

/// Parse a dotenv-format string into a HashMap.
///
/// Rules:
/// - Empty lines and lines starting with `#` are ignored
/// - Split on the first `=` sign (VALUE may contain `=`)
/// - Trim whitespace from KEY and VALUE
/// - Strip surrounding single or double quotes from VALUE
pub fn parse_dotenv(content: &str) -> HashMap<String, String> {
    let mut env = HashMap::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Split on the first '='
        if let Some(pos) = trimmed.find('=') {
            let key = trimmed[..pos].trim();
            let mut value = trimmed[pos + 1..].trim();

            // Strip surrounding quotes
            if ((value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\'')))
                && value.len() >= 2
            {
                value = &value[1..value.len() - 1];
            }

            if !key.is_empty() {
                env.insert(key.to_string(), value.to_string());
            }
        }
    }

    env
}

/// Serialize a HashMap into dotenv-format string.
///
/// Keys are sorted alphabetically for deterministic output.
/// Values containing spaces, quotes, or `#` are wrapped in double quotes.
pub fn serialize_dotenv(env_vars: &HashMap<String, String>) -> String {
    let mut lines = Vec::new();

    let mut keys: Vec<&String> = env_vars.keys().collect();
    keys.sort();

    for key in keys {
        let value = &env_vars[key];
        // Wrap in double quotes if value contains special characters
        if value.contains(' ') || value.contains('#') || value.contains('\'') || value.contains('"')
        {
            let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
            lines.push(format!("{key}=\"{escaped}\""));
        } else {
            lines.push(format!("{key}={value}"));
        }
    }

    if lines.is_empty() {
        String::new()
    } else {
        lines.join("\n") + "\n"
    }
}

/// Read `.env` file from a skill directory.
///
/// Returns an empty HashMap if the file does not exist or cannot be read.
pub fn read_dotenv(skill_dir: &Path) -> HashMap<String, String> {
    let env_path = skill_dir.join(".env");
    match std::fs::read_to_string(&env_path) {
        Ok(content) => parse_dotenv(&content),
        Err(_) => HashMap::new(),
    }
}

/// Write `.env` file to a skill directory.
///
/// If `env_vars` is empty, the `.env` file is deleted (if it exists).
pub fn write_dotenv(skill_dir: &Path, env_vars: &HashMap<String, String>) -> std::io::Result<()> {
    let env_path = skill_dir.join(".env");

    if env_vars.is_empty() {
        // Remove the file if it exists
        match std::fs::remove_file(&env_path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    } else {
        let content = serialize_dotenv(env_vars);
        std::fs::write(&env_path, content)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn test_parse_empty() {
        let result = parse_dotenv("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_comments_and_blank_lines() {
        let content = "# This is a comment\n\n  # Another comment\n  \n";
        let result = parse_dotenv(content);
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_simple() {
        let content = "API_KEY=abc123\nDB_HOST=localhost";
        let result = parse_dotenv(content);
        assert_eq!(result.get("API_KEY").unwrap(), "abc123");
        assert_eq!(result.get("DB_HOST").unwrap(), "localhost");
    }

    #[test]
    fn test_parse_with_double_quotes() {
        let content = "SECRET=\"hello world\"";
        let result = parse_dotenv(content);
        assert_eq!(result.get("SECRET").unwrap(), "hello world");
    }

    #[test]
    fn test_parse_with_single_quotes() {
        let content = "SECRET='hello world'";
        let result = parse_dotenv(content);
        assert_eq!(result.get("SECRET").unwrap(), "hello world");
    }

    #[test]
    fn test_parse_value_with_equals() {
        let content = "CONNECTION=postgres://user:pass@host/db?opt=val";
        let result = parse_dotenv(content);
        assert_eq!(
            result.get("CONNECTION").unwrap(),
            "postgres://user:pass@host/db?opt=val"
        );
    }

    #[test]
    fn test_parse_whitespace_handling() {
        let content = "  KEY  =  value  ";
        let result = parse_dotenv(content);
        assert_eq!(result.get("KEY").unwrap(), "value");
    }

    #[test]
    fn test_parse_empty_value() {
        let content = "EMPTY_VAR=";
        let result = parse_dotenv(content);
        assert_eq!(result.get("EMPTY_VAR").unwrap(), "");
    }

    #[test]
    fn test_serialize_simple() {
        let mut env = HashMap::new();
        env.insert("B_KEY".to_string(), "value_b".to_string());
        env.insert("A_KEY".to_string(), "value_a".to_string());
        let result = serialize_dotenv(&env);
        assert_eq!(result, "A_KEY=value_a\nB_KEY=value_b\n");
    }

    #[test]
    fn test_serialize_with_special_chars() {
        let mut env = HashMap::new();
        env.insert("KEY".to_string(), "hello world".to_string());
        let result = serialize_dotenv(&env);
        assert_eq!(result, "KEY=\"hello world\"\n");
    }

    #[test]
    fn test_serialize_empty() {
        let env = HashMap::new();
        let result = serialize_dotenv(&env);
        assert_eq!(result, "");
    }

    #[test]
    fn test_roundtrip() {
        let mut original = HashMap::new();
        original.insert("API_KEY".to_string(), "sk-1234567890".to_string());
        original.insert("DB_URL".to_string(), "postgres://localhost/db".to_string());
        original.insert("SIMPLE".to_string(), "value".to_string());

        let serialized = serialize_dotenv(&original);
        let parsed = parse_dotenv(&serialized);
        assert_eq!(original, parsed);
    }

    #[test]
    fn test_read_nonexistent_file() {
        let result = read_dotenv(Path::new("/nonexistent/path"));
        assert!(result.is_empty());
    }

    #[test]
    fn test_write_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let mut env = HashMap::new();
        env.insert("TEST_KEY".to_string(), "test_value".to_string());

        write_dotenv(dir.path(), &env).unwrap();
        let result = read_dotenv(dir.path());
        assert_eq!(result.get("TEST_KEY").unwrap(), "test_value");
    }

    #[test]
    fn test_write_empty_deletes_file() {
        let dir = tempfile::tempdir().unwrap();

        // Write a non-empty file first
        let mut env = HashMap::new();
        env.insert("KEY".to_string(), "val".to_string());
        write_dotenv(dir.path(), &env).unwrap();
        assert!(dir.path().join(".env").exists());

        // Write empty to delete
        write_dotenv(dir.path(), &HashMap::new()).unwrap();
        assert!(!dir.path().join(".env").exists());
    }
}
