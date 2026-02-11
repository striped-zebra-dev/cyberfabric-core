use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Shared enums
// ---------------------------------------------------------------------------

/// Hierarchical configuration sharing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SharingMode {
    #[default]
    Private,
    Inherit,
    Enforce,
}

// ---------------------------------------------------------------------------
// AuthConfig
// ---------------------------------------------------------------------------

/// Authentication plugin configuration for an upstream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthConfig {
    /// GTS identifier of the auth plugin type.
    #[serde(rename = "type")]
    pub plugin_type: String,
    #[serde(default)]
    pub sharing: SharingMode,
    /// Plugin-specific configuration (schema varies by plugin type).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// HeadersConfig
// ---------------------------------------------------------------------------

/// Header transformation rules for request and response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct HeadersConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<RequestHeaderRules>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<ResponseHeaderRules>,
}

/// Header transformation rules for outbound requests.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RequestHeaderRules {
    /// Headers to set (overwrite if exists).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub set: HashMap<String, String>,
    /// Headers to add (append, allow duplicates).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub add: HashMap<String, String>,
    /// Header names to remove from inbound request.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove: Vec<String>,
    /// Which inbound headers to forward to upstream.
    #[serde(default)]
    pub passthrough: PassthroughMode,
    /// Headers to forward when passthrough is `allowlist`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub passthrough_allowlist: Vec<String>,
}

/// Header transformation rules for upstream responses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ResponseHeaderRules {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub set: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub add: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove: Vec<String>,
}

/// Controls which inbound headers are forwarded to upstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PassthroughMode {
    #[default]
    None,
    Allowlist,
    All,
}

// ---------------------------------------------------------------------------
// RateLimitConfig
// ---------------------------------------------------------------------------

/// Rate limiting configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RateLimitConfig {
    #[serde(default)]
    pub sharing: SharingMode,
    #[serde(default)]
    pub algorithm: RateLimitAlgorithm,
    pub sustained: SustainedRate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub burst: Option<BurstConfig>,
    #[serde(default)]
    pub scope: RateLimitScope,
    #[serde(default)]
    pub strategy: RateLimitStrategy,
    #[serde(default = "default_cost")]
    pub cost: u32,
}

fn default_cost() -> u32 {
    1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RateLimitAlgorithm {
    #[default]
    TokenBucket,
    SlidingWindow,
}

/// Sustained rate configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SustainedRate {
    /// Tokens replenished per window.
    pub rate: u32,
    #[serde(default)]
    pub window: Window,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Window {
    #[default]
    Second,
    Minute,
    Hour,
    Day,
}

/// Burst capacity configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BurstConfig {
    /// Maximum burst size. Defaults to sustained.rate if not specified.
    pub capacity: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RateLimitScope {
    Global,
    #[default]
    Tenant,
    User,
    Ip,
    Route,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RateLimitStrategy {
    #[default]
    Reject,
    Queue,
    Degrade,
}

// ---------------------------------------------------------------------------
// PluginsConfig
// ---------------------------------------------------------------------------

/// Plugin chain configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PluginsConfig {
    #[serde(default)]
    pub sharing: SharingMode,
    /// Plugin references: GTS identifiers (builtin) or UUIDs (custom).
    #[serde(default)]
    pub items: Vec<String>,
}
