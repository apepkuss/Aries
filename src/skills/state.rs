//! Persistence for skill enabled/disabled state.
//!
//! Stores disabled skill names in `~/.moss/skills-state.json`.
//! File absence or empty = all skills enabled.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{config::app_home_dir, skills::error::SkillResult};

/// JSON structure for the state file
#[derive(Debug, Serialize, Deserialize, Default)]
struct SkillsState {
    #[serde(default)]
    disabled_skills: Vec<String>,
}

/// Return the path to `~/.moss/skills-state.json`
pub fn state_file_path() -> PathBuf {
    app_home_dir().join("skills-state.json")
}

/// Load the set of disabled skill names from disk.
///
/// Returns an empty set if the file does not exist or cannot be parsed.
pub fn load_disabled_skills(path: &Path) -> HashSet<String> {
    match std::fs::read_to_string(path) {
        Ok(content) => match serde_json::from_str::<SkillsState>(&content) {
            Ok(state) => state.disabled_skills.into_iter().collect(),
            Err(e) => {
                tracing::warn!("Failed to parse skills state file: {}", e);
                HashSet::new()
            }
        },
        Err(_) => HashSet::new(),
    }
}

/// Persist the set of disabled skill names to disk.
///
/// Uses atomic write (write to temp file, then rename).
/// If `disabled` is empty, deletes the state file.
pub fn save_disabled_skills(path: &Path, disabled: &HashSet<String>) -> SkillResult<()> {
    if disabled.is_empty() {
        // Remove file if all skills are enabled
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        return Ok(());
    }

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut sorted: Vec<&String> = disabled.iter().collect();
    sorted.sort();

    let state = SkillsState {
        disabled_skills: sorted.into_iter().cloned().collect(),
    };

    let json = serde_json::to_string_pretty(&state).map_err(std::io::Error::other)?;

    // Atomic write: temp file + rename
    let tmp_path = path.with_extension("json.tmp");
    std::fs::write(&tmp_path, json)?;
    std::fs::rename(&tmp_path, path)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_nonexistent_file() {
        let result = load_disabled_skills(Path::new("/nonexistent/skills-state.json"));
        assert!(result.is_empty());
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("skills-state.json");

        let mut disabled = HashSet::new();
        disabled.insert("skill-b".to_string());
        disabled.insert("skill-a".to_string());

        save_disabled_skills(&path, &disabled).unwrap();
        let loaded = load_disabled_skills(&path);

        assert_eq!(loaded, disabled);
    }

    #[test]
    fn test_save_empty_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("skills-state.json");

        // Write something first
        let mut disabled = HashSet::new();
        disabled.insert("skill-a".to_string());
        save_disabled_skills(&path, &disabled).unwrap();
        assert!(path.exists());

        // Save empty set — file should be removed
        save_disabled_skills(&path, &HashSet::new()).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn test_load_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("skills-state.json");
        std::fs::write(&path, "not valid json").unwrap();

        let result = load_disabled_skills(&path);
        assert!(result.is_empty());
    }

    #[test]
    fn test_save_sorted_output() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("skills-state.json");

        let mut disabled = HashSet::new();
        disabled.insert("z-skill".to_string());
        disabled.insert("a-skill".to_string());
        disabled.insert("m-skill".to_string());

        save_disabled_skills(&path, &disabled).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let state: SkillsState = serde_json::from_str(&content).unwrap();
        assert_eq!(state.disabled_skills, vec!["a-skill", "m-skill", "z-skill"]);
    }
}
