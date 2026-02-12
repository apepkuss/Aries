//! Artifact data types and structures
//!
//! Core type definitions for the Artifacts management system.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ============================================================================
// Artifact Type
// ============================================================================

/// Artifact type enumeration
///
/// Defines the type of content stored in an artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactType {
    // ========== Text Types ==========
    /// Code file with language identifier
    Code { language: String },
    /// HTML content
    Html,
    /// SVG graphics
    Svg,
    /// Markdown document
    Markdown,
    /// JSON data
    Json,
    /// Plain text
    #[default]
    Text,

    // ========== Binary Types (Phase 6) ==========
    /// Image file
    Image { format: String },
    /// PDF document
    Pdf,
    /// Audio file
    Audio { format: String },
    /// Video file
    Video { format: String },
    /// Generic binary file
    Binary { mime_type: String },

    // ========== Other ==========
    /// Other types
    Other { mime_type: String },
}

impl ArtifactType {
    /// Get MIME type for this artifact type
    pub fn mime_type(&self) -> &str {
        match self {
            // Text types
            ArtifactType::Code { language } => match language.as_str() {
                "rust" => "text/x-rust",
                "python" => "text/x-python",
                "javascript" | "js" => "text/javascript",
                "typescript" | "ts" => "text/typescript",
                "go" => "text/x-go",
                "java" => "text/x-java",
                "c" => "text/x-c",
                "cpp" | "c++" => "text/x-c++",
                "html" => "text/html",
                "css" => "text/css",
                "sql" => "text/x-sql",
                "shell" | "bash" | "sh" => "text/x-shellscript",
                "yaml" | "yml" => "text/yaml",
                "toml" => "text/toml",
                _ => "text/plain",
            },
            ArtifactType::Html => "text/html",
            ArtifactType::Svg => "image/svg+xml",
            ArtifactType::Markdown => "text/markdown",
            ArtifactType::Json => "application/json",
            ArtifactType::Text => "text/plain",

            // Binary types (Phase 6)
            ArtifactType::Image { format } => match format.as_str() {
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "gif" => "image/gif",
                "webp" => "image/webp",
                "ico" => "image/x-icon",
                "bmp" => "image/bmp",
                _ => "application/octet-stream",
            },
            ArtifactType::Pdf => "application/pdf",
            ArtifactType::Audio { format } => match format.as_str() {
                "mp3" => "audio/mpeg",
                "wav" => "audio/wav",
                "ogg" => "audio/ogg",
                "flac" => "audio/flac",
                "aac" => "audio/aac",
                "webm" => "audio/webm",
                _ => "application/octet-stream",
            },
            ArtifactType::Video { format } => match format.as_str() {
                "mp4" => "video/mp4",
                "webm" => "video/webm",
                "ogg" => "video/ogg",
                "avi" => "video/x-msvideo",
                "mov" => "video/quicktime",
                _ => "application/octet-stream",
            },
            ArtifactType::Binary { mime_type } => mime_type,

            // Other types
            ArtifactType::Other { mime_type } => mime_type,
        }
    }

    /// Check if this is a binary type
    #[allow(dead_code)]
    pub fn is_binary(&self) -> bool {
        matches!(
            self,
            ArtifactType::Image { .. }
                | ArtifactType::Pdf
                | ArtifactType::Audio { .. }
                | ArtifactType::Video { .. }
                | ArtifactType::Binary { .. }
        )
    }

    /// Infer artifact type from file extension
    #[allow(dead_code)]
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            // Code files
            "rs" => ArtifactType::Code {
                language: "rust".into(),
            },
            "py" => ArtifactType::Code {
                language: "python".into(),
            },
            "js" => ArtifactType::Code {
                language: "javascript".into(),
            },
            "ts" => ArtifactType::Code {
                language: "typescript".into(),
            },
            "go" => ArtifactType::Code {
                language: "go".into(),
            },
            "java" => ArtifactType::Code {
                language: "java".into(),
            },
            "c" => ArtifactType::Code {
                language: "c".into(),
            },
            "cpp" | "cc" | "cxx" => ArtifactType::Code {
                language: "cpp".into(),
            },
            "css" => ArtifactType::Code {
                language: "css".into(),
            },
            "sql" => ArtifactType::Code {
                language: "sql".into(),
            },
            "sh" | "bash" => ArtifactType::Code {
                language: "shell".into(),
            },
            "yaml" | "yml" => ArtifactType::Code {
                language: "yaml".into(),
            },
            "toml" => ArtifactType::Code {
                language: "toml".into(),
            },

            // Text files
            "html" | "htm" => ArtifactType::Html,
            "svg" => ArtifactType::Svg,
            "md" | "markdown" => ArtifactType::Markdown,
            "json" => ArtifactType::Json,
            "txt" => ArtifactType::Text,

            // Image files (Phase 6)
            "png" => ArtifactType::Image {
                format: "png".into(),
            },
            "jpg" | "jpeg" => ArtifactType::Image {
                format: "jpeg".into(),
            },
            "gif" => ArtifactType::Image {
                format: "gif".into(),
            },
            "webp" => ArtifactType::Image {
                format: "webp".into(),
            },
            "ico" => ArtifactType::Image {
                format: "ico".into(),
            },
            "bmp" => ArtifactType::Image {
                format: "bmp".into(),
            },

            // PDF (Phase 6)
            "pdf" => ArtifactType::Pdf,

            // Audio files (Phase 6)
            "mp3" => ArtifactType::Audio {
                format: "mp3".into(),
            },
            "wav" => ArtifactType::Audio {
                format: "wav".into(),
            },
            "ogg" => ArtifactType::Audio {
                format: "ogg".into(),
            },
            "flac" => ArtifactType::Audio {
                format: "flac".into(),
            },
            "aac" => ArtifactType::Audio {
                format: "aac".into(),
            },

            // Video files (Phase 6)
            "mp4" => ArtifactType::Video {
                format: "mp4".into(),
            },
            "webm" => ArtifactType::Video {
                format: "webm".into(),
            },
            "avi" => ArtifactType::Video {
                format: "avi".into(),
            },
            "mov" => ArtifactType::Video {
                format: "mov".into(),
            },

            _ => ArtifactType::Text,
        }
    }
}

// ============================================================================
// Artifact
// ============================================================================

/// Main Artifact structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    /// Unique identifier (UUID)
    pub id: String,
    /// Associated conversation ID
    pub conversation_id: String,
    /// User ID (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last update timestamp
    pub updated_at: DateTime<Utc>,
    /// Artifact title
    pub title: String,
    /// Description (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Artifact type
    pub artifact_type: ArtifactType,
    /// Content size in bytes
    pub size: u64,
    /// Soft delete flag
    #[serde(default)]
    pub is_deleted: bool,
    /// Download URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

// ============================================================================
// Request Types
// ============================================================================

/// Request to create a new artifact
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateArtifactRequest {
    /// Associated conversation ID
    pub conversation_id: String,
    /// Title
    pub title: String,
    /// Description (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Artifact type
    pub artifact_type: ArtifactType,
    /// Content
    pub content: String,
}

impl Default for CreateArtifactRequest {
    fn default() -> Self {
        Self {
            conversation_id: String::new(),
            title: String::new(),
            description: None,
            artifact_type: ArtifactType::Text,
            content: String::new(),
        }
    }
}

/// Request to update an existing artifact
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateArtifactRequest {
    /// New title (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// New description (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// New content (optional, overwrites existing content if provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

// ============================================================================
// Response Types
// ============================================================================

/// Artifact list response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactListResponse {
    /// List of artifacts
    pub artifacts: Vec<Artifact>,
    /// Total count
    pub total: usize,
    /// Whether there are more results
    pub has_more: bool,
}

/// Artifact detail response (includes content)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactDetailResponse {
    /// Artifact metadata
    #[serde(flatten)]
    pub artifact: Artifact,
    /// Content
    pub content: String,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_artifact_type_mime_types() {
        assert_eq!(
            ArtifactType::Code {
                language: "rust".into()
            }
            .mime_type(),
            "text/x-rust"
        );
        assert_eq!(ArtifactType::Html.mime_type(), "text/html");
        assert_eq!(ArtifactType::Json.mime_type(), "application/json");
        assert_eq!(ArtifactType::Svg.mime_type(), "image/svg+xml");
        assert_eq!(ArtifactType::Markdown.mime_type(), "text/markdown");
        assert_eq!(ArtifactType::Pdf.mime_type(), "application/pdf");
        assert_eq!(
            ArtifactType::Image {
                format: "png".into()
            }
            .mime_type(),
            "image/png"
        );
    }

    #[test]
    fn test_artifact_type_from_extension() {
        assert!(matches!(
            ArtifactType::from_extension("rs"),
            ArtifactType::Code { language } if language == "rust"
        ));
        assert!(matches!(
            ArtifactType::from_extension("html"),
            ArtifactType::Html
        ));
        assert!(matches!(
            ArtifactType::from_extension("json"),
            ArtifactType::Json
        ));
        assert!(matches!(
            ArtifactType::from_extension("pdf"),
            ArtifactType::Pdf
        ));
        assert!(matches!(
            ArtifactType::from_extension("png"),
            ArtifactType::Image { format } if format == "png"
        ));
    }

    #[test]
    fn test_artifact_type_is_binary() {
        assert!(!ArtifactType::Text.is_binary());
        assert!(!ArtifactType::Html.is_binary());
        assert!(
            !ArtifactType::Code {
                language: "rust".into()
            }
            .is_binary()
        );

        assert!(ArtifactType::Pdf.is_binary());
        assert!(
            ArtifactType::Image {
                format: "png".into()
            }
            .is_binary()
        );
        assert!(
            ArtifactType::Audio {
                format: "mp3".into()
            }
            .is_binary()
        );
        assert!(
            ArtifactType::Video {
                format: "mp4".into()
            }
            .is_binary()
        );
    }

    #[test]
    fn test_artifact_serialization() {
        let artifact = Artifact {
            id: "art_123".to_string(),
            conversation_id: "conv_456".to_string(),
            user_id: Some("user_789".to_string()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            title: "main.rs".to_string(),
            description: Some("Main entry point".to_string()),
            artifact_type: ArtifactType::Code {
                language: "rust".into(),
            },
            size: 100,
            is_deleted: false,
            url: Some("/v1/artifacts/art_123/download".to_string()),
        };

        let json = serde_json::to_string(&artifact).unwrap();
        assert!(json.contains("art_123"));
        assert!(json.contains("main.rs"));
        assert!(json.contains("rust"));

        let deserialized: Artifact = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, artifact.id);
        assert_eq!(deserialized.title, artifact.title);
    }

    #[test]
    fn test_create_artifact_request_serialization() {
        let request = CreateArtifactRequest {
            conversation_id: "conv_123".to_string(),
            title: "test.py".to_string(),
            description: Some("Test script".to_string()),
            artifact_type: ArtifactType::Code {
                language: "python".into(),
            },
            content: "print('hello')".to_string(),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("conv_123"));
        assert!(json.contains("test.py"));
        assert!(json.contains("python"));

        let deserialized: CreateArtifactRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.title, "test.py");
    }

    #[test]
    fn test_update_artifact_request_optional_fields() {
        let request = UpdateArtifactRequest {
            title: Some("new_title.rs".to_string()),
            description: None,
            content: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("new_title.rs"));
        // Optional None fields should be omitted
        assert!(!json.contains("description"));
        assert!(!json.contains("content"));
    }

    #[test]
    fn test_artifact_list_response_serialization() {
        let response = ArtifactListResponse {
            artifacts: vec![],
            total: 0,
            has_more: false,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"artifacts\":[]"));
        assert!(json.contains("\"total\":0"));
        assert!(json.contains("\"has_more\":false"));
    }
}
