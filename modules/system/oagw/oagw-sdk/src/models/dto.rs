use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::config::{AuthConfig, HeadersConfig, PluginsConfig, RateLimitConfig};
use super::route::MatchRules;
use super::upstream::Server;

// ---------------------------------------------------------------------------
// Upstream DTOs
// ---------------------------------------------------------------------------

/// Request body for creating an upstream. No `id` or `tenant_id` (server-assigned).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateUpstreamRequest {
    pub server: Server,
    pub protocol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<HeadersConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugins: Option<PluginsConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<RateLimitConfig>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// Request body for updating an upstream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct UpdateUpstreamRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<Server>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<HeadersConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugins: Option<PluginsConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<RateLimitConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

/// Response body for an upstream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpstreamResponse {
    /// GTS format identifier.
    pub id: String,
    pub tenant_id: Uuid,
    pub alias: String,
    pub server: Server,
    pub protocol: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<HeadersConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugins: Option<PluginsConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<RateLimitConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

// ---------------------------------------------------------------------------
// Route DTOs
// ---------------------------------------------------------------------------

/// Request body for creating a route. No `id` or `tenant_id` (server-assigned).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateRouteRequest {
    pub upstream_id: Uuid,
    #[serde(rename = "match")]
    pub match_rules: MatchRules,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugins: Option<PluginsConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<RateLimitConfig>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// Request body for updating a route.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct UpdateRouteRequest {
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "match")]
    pub match_rules: Option<MatchRules>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugins: Option<PluginsConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<RateLimitConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

/// Response body for a route.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteResponse {
    /// GTS format identifier.
    pub id: String,
    pub tenant_id: Uuid,
    pub upstream_id: Uuid,
    #[serde(rename = "match")]
    pub match_rules: MatchRules,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugins: Option<PluginsConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<RateLimitConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub priority: i32,
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}
