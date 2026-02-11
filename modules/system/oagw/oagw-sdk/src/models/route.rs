use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::config::{PluginsConfig, RateLimitConfig};

/// HTTP methods supported by route matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
}

/// How path_suffix from the proxy URL is handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PathSuffixMode {
    Disabled,
    #[default]
    Append,
}

/// HTTP-protocol match rules for a route.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HttpMatch {
    /// At least one method required.
    pub methods: Vec<HttpMethod>,
    /// Path prefix (must start with `/`).
    pub path: String,
    /// Allowed query parameters. Empty = allow none.
    #[serde(default)]
    pub query_allowlist: Vec<String>,
    #[serde(default)]
    pub path_suffix_mode: PathSuffixMode,
}

/// gRPC-protocol match rules for a route (future use).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GrpcMatch {
    pub service: String,
    pub method: String,
}

/// Protocol-scoped matching rules. Exactly one of `http` or `grpc` must be present.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatchRules {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<HttpMatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grpc: Option<GrpcMatch>,
}

/// A route mapping inbound requests to an upstream endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Route {
    pub id: Uuid,
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
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_route() -> Route {
        Route {
            id: Uuid::nil(),
            tenant_id: Uuid::nil(),
            upstream_id: Uuid::nil(),
            match_rules: MatchRules {
                http: Some(HttpMatch {
                    methods: vec![HttpMethod::Post],
                    path: "/v1/chat/completions".into(),
                    query_allowlist: vec![],
                    path_suffix_mode: PathSuffixMode::Append,
                }),
                grpc: None,
            },
            plugins: None,
            rate_limit: None,
            tags: vec![],
            priority: 0,
            enabled: true,
        }
    }

    #[test]
    fn match_field_serializes_as_match() {
        let r = make_route();
        let v: serde_json::Value = serde_json::to_value(&r).unwrap();
        assert!(v.get("match").is_some());
        assert!(v.get("match_rules").is_none());
    }

    #[test]
    fn defaults_applied() {
        let json = r#"{
            "id": "00000000-0000-0000-0000-000000000000",
            "tenant_id": "00000000-0000-0000-0000-000000000000",
            "upstream_id": "00000000-0000-0000-0000-000000000000",
            "match": {
                "http": {
                    "methods": ["POST"],
                    "path": "/v1/chat/completions"
                }
            }
        }"#;
        let r: Route = serde_json::from_str(json).unwrap();
        assert!(r.enabled);
        assert_eq!(r.priority, 0);
        let http = r.match_rules.http.unwrap();
        assert_eq!(http.path_suffix_mode, PathSuffixMode::Append);
        assert!(http.query_allowlist.is_empty());
    }

    #[test]
    fn serde_round_trip() {
        let r = make_route();
        let json = serde_json::to_string(&r).unwrap();
        let r2: Route = serde_json::from_str(&json).unwrap();
        assert_eq!(r, r2);
    }

    #[test]
    fn http_method_serializes_uppercase() {
        let json = serde_json::to_string(&HttpMethod::Post).unwrap();
        assert_eq!(json, r#""POST""#);
    }
}
