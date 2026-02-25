//! ClawHub API client for OpenClaw's official skill marketplace
//!
//! Provides API integration for:
//! - Searching skills (semantic vector search)
//! - Browsing skills (paginated, sortable)
//! - Getting skill details
//! - Downloading skill packages

use serde::{Deserialize, Serialize};

use crate::error::{ServerError, ServerResult};

/// Base URL for ClawHub API
const CLAWHUB_API_BASE: &str = "https://clawhub.ai/api/v1";

/// Default User-Agent for ClawHub requests
const USER_AGENT: &str = "moss/0.8.2";

/// ClawHub API client
pub struct ClawHubClient {
    client: reqwest::Client,
}

/// Skill information from ClawHub
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClawHubSkill {
    /// Unique slug identifier (e.g., "web-researcher")
    pub slug: String,
    /// Display name
    #[serde(default, rename = "displayName")]
    pub display_name: Option<String>,
    /// Short description from SKILL.md frontmatter
    #[serde(default)]
    pub summary: Option<String>,
    /// Skill owner
    #[serde(default)]
    pub owner: Option<ClawHubOwner>,
    /// Download/star statistics
    #[serde(default)]
    pub stats: Option<ClawHubStats>,
    /// Latest published version
    #[serde(default, rename = "latestVersion")]
    pub latest_version: Option<ClawHubVersion>,
    /// Badges (official, highlighted, etc.)
    #[serde(default)]
    pub badges: Option<ClawHubBadges>,
}

/// Skill owner information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClawHubOwner {
    /// GitHub handle
    pub handle: String,
    /// Display name
    #[serde(default)]
    pub name: Option<String>,
}

/// Skill statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClawHubStats {
    /// Total downloads
    #[serde(default)]
    pub downloads: u64,
    /// Star count
    #[serde(default)]
    pub stars: u64,
}

/// Version information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClawHubVersion {
    /// Semver version string
    pub version: String,
    /// Parsed metadata from SKILL.md (includes requires, etc.)
    #[serde(default)]
    pub parsed: Option<serde_json::Value>,
}

/// Badge flags
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClawHubBadges {
    /// Official skill by ClawHub team
    #[serde(default)]
    pub official: Option<serde_json::Value>,
    /// Highlighted/recommended skill
    #[serde(default)]
    pub highlighted: Option<serde_json::Value>,
    /// Deprecated skill
    #[serde(default)]
    pub deprecated: Option<serde_json::Value>,
}

/// Paginated browse response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClawHubBrowseResponse {
    /// List of skills
    pub skills: Vec<ClawHubSkill>,
    /// Cursor for next page (None if last page)
    #[serde(default)]
    pub cursor: Option<String>,
}

/// API response wrapper for search endpoint
#[derive(Debug, Deserialize)]
struct SearchApiResponse {
    #[serde(default)]
    results: Vec<ClawHubSkill>,
}

/// API response wrapper for list endpoint
#[derive(Debug, Deserialize)]
struct ListApiResponse {
    #[serde(default)]
    items: Vec<ClawHubSkill>,
    #[serde(default, rename = "nextCursor")]
    cursor: Option<String>,
}

impl ClawHubClient {
    /// Create a new ClawHub API client
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self { client }
    }

    /// Search skills using semantic vector search
    pub async fn search(&self, query: &str, limit: usize) -> ServerResult<Vec<ClawHubSkill>> {
        let url = format!("{}/search", CLAWHUB_API_BASE);

        let response = self
            .client
            .get(&url)
            .query(&[("q", query), ("limit", &limit.to_string())])
            .send()
            .await
            .map_err(|e| ServerError::Operation(format!("Failed to search ClawHub: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ServerError::Operation(format!(
                "ClawHub search API error ({}): {}",
                status, body
            )));
        }

        // ClawHub search may return results directly as an array or wrapped
        let text = response.text().await.map_err(|e| {
            ServerError::Operation(format!("Failed to read ClawHub response: {}", e))
        })?;

        // Try parsing as array first, then as wrapped response
        if let Ok(skills) = serde_json::from_str::<Vec<ClawHubSkill>>(&text) {
            return Ok(skills);
        }

        if let Ok(resp) = serde_json::from_str::<SearchApiResponse>(&text) {
            return Ok(resp.results);
        }

        Err(ServerError::Operation(format!(
            "Failed to parse ClawHub search response: {}",
            &text[..text.len().min(200)]
        )))
    }

    /// Browse skills with pagination and sorting
    pub async fn browse(
        &self,
        limit: usize,
        cursor: Option<&str>,
        sort: Option<&str>,
    ) -> ServerResult<ClawHubBrowseResponse> {
        let url = format!("{}/skills", CLAWHUB_API_BASE);

        let mut request = self
            .client
            .get(&url)
            .query(&[("limit", &limit.to_string())]);

        if let Some(c) = cursor {
            request = request.query(&[("cursor", c)]);
        }
        if let Some(s) = sort {
            request = request.query(&[("sort", s)]);
        }

        let response = request
            .send()
            .await
            .map_err(|e| ServerError::Operation(format!("Failed to browse ClawHub: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ServerError::Operation(format!(
                "ClawHub browse API error ({}): {}",
                status, body
            )));
        }

        let text = response.text().await.map_err(|e| {
            ServerError::Operation(format!("Failed to read ClawHub response: {}", e))
        })?;

        // Try parsing as paginated response with items
        if let Ok(resp) = serde_json::from_str::<ListApiResponse>(&text) {
            return Ok(ClawHubBrowseResponse {
                skills: resp.items,
                cursor: resp.cursor,
            });
        }

        // Try parsing as direct browse response
        if let Ok(resp) = serde_json::from_str::<ClawHubBrowseResponse>(&text) {
            return Ok(resp);
        }

        Err(ServerError::Operation(format!(
            "Failed to parse ClawHub browse response: {}",
            &text[..text.len().min(200)]
        )))
    }

    /// Get detailed information about a specific skill
    pub async fn get_skill(&self, slug: &str) -> ServerResult<ClawHubSkill> {
        let url = format!("{}/skills/{}", CLAWHUB_API_BASE, slug);

        let response = self.client.get(&url).send().await.map_err(|e| {
            ServerError::Operation(format!("Failed to get ClawHub skill '{}': {}", slug, e))
        })?;

        if !response.status().is_success() {
            let status = response.status();
            if status == reqwest::StatusCode::NOT_FOUND {
                return Err(ServerError::Operation(format!(
                    "Skill '{}' not found on ClawHub",
                    slug
                )));
            }
            let body = response.text().await.unwrap_or_default();
            return Err(ServerError::Operation(format!(
                "ClawHub API error ({}): {}",
                status, body
            )));
        }

        response.json().await.map_err(|e| {
            ServerError::Operation(format!("Failed to parse ClawHub skill response: {}", e))
        })
    }

    /// Download a skill package (zip format)
    pub async fn download(&self, slug: &str, version: Option<&str>) -> ServerResult<bytes::Bytes> {
        let url = format!("{}/download", CLAWHUB_API_BASE);

        let mut request = self.client.get(&url).query(&[("slug", slug)]);

        if let Some(ver) = version {
            request = request.query(&[("version", ver)]);
        }

        let response = request.send().await.map_err(|e| {
            ServerError::Operation(format!(
                "Failed to download skill '{}' from ClawHub: {}",
                slug, e
            ))
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ServerError::Operation(format!(
                "ClawHub download error ({}): {}",
                status, body
            )));
        }

        response.bytes().await.map_err(|e| {
            ServerError::Operation(format!("Failed to read skill package from ClawHub: {}", e))
        })
    }
}

impl Default for ClawHubClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clawhub_skill_deserialize() {
        let json = r#"{
            "slug": "web-researcher",
            "displayName": "Web Researcher",
            "summary": "Research the web for information",
            "owner": { "handle": "testuser" },
            "stats": { "downloads": 1234, "stars": 56 },
            "latestVersion": { "version": "1.2.0" }
        }"#;

        let skill: ClawHubSkill = serde_json::from_str(json).unwrap();
        assert_eq!(skill.slug, "web-researcher");
        assert_eq!(skill.display_name.as_deref(), Some("Web Researcher"));
        assert_eq!(skill.stats.as_ref().unwrap().downloads, 1234);
        assert_eq!(skill.stats.as_ref().unwrap().stars, 56);
        assert_eq!(skill.latest_version.as_ref().unwrap().version, "1.2.0");
    }

    #[test]
    fn test_clawhub_skill_deserialize_minimal() {
        let json = r#"{ "slug": "minimal-skill" }"#;

        let skill: ClawHubSkill = serde_json::from_str(json).unwrap();
        assert_eq!(skill.slug, "minimal-skill");
        assert!(skill.display_name.is_none());
        assert!(skill.summary.is_none());
        assert!(skill.owner.is_none());
        assert!(skill.stats.is_none());
    }

    #[test]
    fn test_clawhub_badges_deserialize() {
        let json = r#"{
            "slug": "official-skill",
            "badges": {
                "official": { "byUserId": "admin", "timestamp": 1234567890 },
                "highlighted": { "byUserId": "admin", "timestamp": 1234567890 }
            }
        }"#;

        let skill: ClawHubSkill = serde_json::from_str(json).unwrap();
        assert!(skill.badges.as_ref().unwrap().official.is_some());
        assert!(skill.badges.as_ref().unwrap().highlighted.is_some());
        assert!(skill.badges.as_ref().unwrap().deprecated.is_none());
    }
}
