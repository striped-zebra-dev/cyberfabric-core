use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::config::{AuthConfig, HeadersConfig, PluginsConfig, RateLimitConfig};
use super::endpoint::Endpoint;

/// Container for upstream server endpoints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Server {
    pub endpoints: Vec<Endpoint>,
}

/// An external upstream service configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Upstream {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub alias: String,
    pub server: Server,
    /// Protocol GTS identifier (e.g. `gts.x.core.oagw.protocol.v1~x.core.http.v1`).
    pub protocol: String,
    #[serde(default = "default_true")]
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

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::endpoint::Scheme;

    fn make_upstream() -> Upstream {
        Upstream {
            id: Uuid::nil(),
            tenant_id: Uuid::nil(),
            alias: "api.openai.com".into(),
            server: Server {
                endpoints: vec![Endpoint {
                    scheme: Scheme::Https,
                    host: "api.openai.com".into(),
                    port: 443,
                }],
            },
            protocol: "gts.x.core.oagw.protocol.v1~x.core.http.v1".into(),
            enabled: true,
            auth: None,
            headers: None,
            plugins: None,
            rate_limit: None,
            tags: vec![],
        }
    }

    #[test]
    fn enabled_defaults_to_true() {
        let json = r#"{
            "id": "00000000-0000-0000-0000-000000000000",
            "tenant_id": "00000000-0000-0000-0000-000000000000",
            "alias": "test",
            "server": {"endpoints": [{"host": "example.com"}]},
            "protocol": "gts.x.core.oagw.protocol.v1~x.core.http.v1"
        }"#;
        let u: Upstream = serde_json::from_str(json).unwrap();
        assert!(u.enabled);
    }

    #[test]
    fn serde_round_trip() {
        let u = make_upstream();
        let json = serde_json::to_string(&u).unwrap();
        let u2: Upstream = serde_json::from_str(&json).unwrap();
        assert_eq!(u, u2);
    }
}
