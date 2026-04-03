//! Skill output artifact management.
//!
//! When a skill script produces raw data output (JSON, text, markdown, images,
//! audio, etc.), this module saves it as a typed artifact file and returns
//! the file path for the LLM to reference — keeping raw content out of the
//! conversation context.
//!
//! **Pass-through rule**: if the trimmed stdout is a single line that resolves
//! to an existing absolute path, it is treated as a file the skill already
//! created and is returned unchanged.

use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use tracing::info;

// ── Content type ─────────────────────────────────────────────────────────────

/// Detected content type of a skill's stdout output.
#[derive(Debug, Clone, PartialEq)]
enum ContentType {
    /// Output is already a path to an existing file — pass through.
    FileRef,
    /// JSON data.
    Json,
    /// PNG image.
    Png,
    /// JPEG image.
    Jpeg,
    /// GIF image.
    Gif,
    /// WebP image.
    Webp,
    /// SVG vector image.
    Svg,
    /// MP3 audio.
    Mp3,
    /// WAV audio.
    Wav,
    /// Markdown text.
    Markdown,
    /// Plain text (fallback).
    Text,
}

impl ContentType {
    fn extension(&self) -> &'static str {
        match self {
            Self::FileRef => "",
            Self::Json => "json",
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Gif => "gif",
            Self::Webp => "webp",
            Self::Svg => "svg",
            Self::Mp3 => "mp3",
            Self::Wav => "wav",
            Self::Markdown => "md",
            Self::Text => "txt",
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::FileRef => "file",
            Self::Json => "JSON",
            Self::Png => "PNG image",
            Self::Jpeg => "JPEG image",
            Self::Gif => "GIF image",
            Self::Webp => "WebP image",
            Self::Svg => "SVG image",
            Self::Mp3 => "MP3 audio",
            Self::Wav => "WAV audio",
            Self::Markdown => "Markdown",
            Self::Text => "plain text",
        }
    }
}

// ── Constants ────────────────────────────────────────────────────────────────

/// Text-based outputs (plain text, JSON, Markdown) at or below this size are
/// returned inline to the LLM context instead of being saved as artifact files.
/// This keeps short, data-oriented outputs (e.g. weather data, API responses)
/// directly available for the model to reason about, while large outputs
/// (e.g. video captions, full documents) are still offloaded to disk.
const INLINE_THRESHOLD: usize = 4096; // 4 KB

// ── Artifact saver ────────────────────────────────────────────────────────────

/// Processes skill script stdout: detects content type, saves as artifact,
/// and returns a replacement string for the LLM context.
pub struct SkillArtifactSaver;

impl SkillArtifactSaver {
    /// Resolves the artifacts directory.
    ///
    /// Priority:
    /// 1. `configured_path` — value from `Config.artifacts.storage_path`
    /// 2. `~/.moss/artifacts/` — hardcoded fallback
    ///
    /// The directory is created if it does not exist.
    fn artifacts_dir(configured_path: Option<&str>) -> Option<PathBuf> {
        let dir = if let Some(p) = configured_path.filter(|s| !s.is_empty()) {
            // Expand leading ~/
            if let Some(rest) = p.strip_prefix("~/") {
                let home = std::env::var("HOME").ok()?;
                PathBuf::from(format!("{}/{}", home, rest))
            } else {
                PathBuf::from(p)
            }
        } else {
            let home = std::env::var("HOME").ok()?;
            PathBuf::from(home).join(".moss").join("artifacts")
        };
        std::fs::create_dir_all(&dir).ok()?;
        Some(dir)
    }

    /// Returns the resolved absolute path if `output` (trimmed) is a single
    /// line pointing to an existing file; `None` otherwise.
    fn detect_file_ref(output: &str) -> Option<String> {
        let trimmed = output.trim();
        if trimmed.contains('\n') {
            return None;
        }
        let path_str = if let Some(rest) = trimmed.strip_prefix("~/") {
            let home = std::env::var("HOME").ok()?;
            format!("{}/{}", home, rest)
        } else if trimmed.starts_with('/') {
            trimmed.to_string()
        } else {
            return None;
        };
        if std::path::Path::new(&path_str).exists() {
            Some(path_str)
        } else {
            None
        }
    }

    /// Detect the content type from raw bytes and the UTF-8 text view.
    fn detect_type(data: &[u8], text: &str) -> ContentType {
        // 1. Existing file path → pass through
        if Self::detect_file_ref(text).is_some() {
            return ContentType::FileRef;
        }

        // 2. Binary magic bytes
        if data.starts_with(b"\x89PNG\r\n\x1a\n") {
            return ContentType::Png;
        }
        if data.starts_with(b"\xFF\xD8\xFF") {
            return ContentType::Jpeg;
        }
        if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
            return ContentType::Gif;
        }
        if data.starts_with(b"RIFF") && data.len() > 11 && &data[8..12] == b"WEBP" {
            return ContentType::Webp;
        }
        // MP3: ID3 tag header or sync-safe frame header (0xFF 0xEx / 0xFF 0xFx)
        if data.starts_with(b"ID3")
            || (data.len() > 1 && data[0] == 0xFF && (data[1] & 0xE0) == 0xE0)
        {
            return ContentType::Mp3;
        }
        if data.starts_with(b"RIFF") && data.len() > 11 && &data[8..12] == b"WAVE" {
            return ContentType::Wav;
        }

        // 3. Text-based detection
        let trimmed = text.trim();

        // SVG (check before JSON — SVG can contain JSON-like attributes)
        if trimmed.contains("<svg") || trimmed.contains("<SVG") {
            return ContentType::Svg;
        }

        // JSON
        if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
            return ContentType::Json;
        }

        // Markdown heuristics
        let has_heading = trimmed.lines().any(|l| l.starts_with('#'));
        let has_bold = trimmed.contains("**");
        let has_code_block = trimmed.contains("```");
        let has_list = trimmed
            .lines()
            .any(|l| l.starts_with("- ") || l.starts_with("* ") || l.starts_with("1. "));
        if has_heading || (has_bold && has_code_block) || has_list {
            return ContentType::Markdown;
        }

        ContentType::Text
    }

    /// Write `data` to `<artifacts_dir>/<safe_skill_name>_<unix_ts>.<ext>`.
    /// Returns the absolute path of the saved file.
    fn save(
        skill_name: &str,
        data: &[u8],
        content_type: &ContentType,
        configured_path: Option<&str>,
    ) -> Result<String, String> {
        let dir = Self::artifacts_dir(configured_path)
            .ok_or_else(|| "cannot determine artifacts directory".to_string())?;

        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let safe_name: String = skill_name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();

        let filename = format!("{}_{}.{}", safe_name, ts, content_type.extension());
        let path = dir.join(&filename);

        std::fs::write(&path, data)
            .map_err(|e| format!("failed to write artifact '{}': {}", path.display(), e))?;

        let path_str = path.to_string_lossy().to_string();
        info!(
            "[artifact] saved skill '{}' output as {} → {}",
            skill_name,
            content_type.label(),
            path_str
        );
        Ok(path_str)
    }

    /// Process skill script stdout.
    ///
    /// - If `stdout` is already a path to an existing file → return unchanged.
    /// - If `stdout` is text-based (plain text, JSON, Markdown) and within
    ///   [`INLINE_THRESHOLD`] → return inline so the LLM can reason about it.
    /// - Otherwise → detect content type, save as `<artifacts_dir>/<skill>_<ts>.<ext>`,
    ///   return a short replacement string for the LLM context.
    ///
    /// `artifacts_dir` should come from `Config.artifacts.storage_path`; if
    /// `None` or empty the default `~/.moss/artifacts/` is used.
    ///
    /// On I/O failure the original `stdout` is returned with a warning appended,
    /// so the LLM always receives *something*.
    pub fn process(skill_name: &str, stdout: &str, artifacts_dir: Option<&str>) -> String {
        let data = stdout.as_bytes();
        let content_type = Self::detect_type(data, stdout);

        if content_type == ContentType::FileRef {
            return stdout.to_string();
        }

        // Small text-based outputs are returned inline for the LLM to use directly.
        let is_text_type = matches!(
            content_type,
            ContentType::Text | ContentType::Json | ContentType::Markdown
        );
        if is_text_type && data.len() <= INLINE_THRESHOLD {
            return stdout.to_string();
        }

        match Self::save(skill_name, data, &content_type, artifacts_dir) {
            Ok(artifact_path) => format!(
                "[Artifact] {} output saved to: {}",
                content_type.label(),
                artifact_path
            ),
            Err(e) => {
                // Saving failed — fall back to raw content to avoid losing data
                format!(
                    "{}\n\n[Warning: artifact save failed: {}]",
                    stdout.trim(),
                    e
                )
            }
        }
    }
}
