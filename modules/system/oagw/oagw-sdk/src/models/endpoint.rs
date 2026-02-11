use serde::{Deserialize, Serialize};

/// Transport scheme for upstream endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scheme {
    Http,
    Https,
    Wss,
    Wt,
    Grpc,
}

impl Default for Scheme {
    fn default() -> Self {
        Self::Https
    }
}

/// A single upstream endpoint (scheme + host + port).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    #[serde(default)]
    pub scheme: Scheme,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

fn default_port() -> u16 {
    443
}

impl Endpoint {
    /// Generate the alias contribution for this endpoint.
    /// Standard ports (80, 443) are omitted; non-standard ports are appended as `:port`.
    #[must_use]
    pub fn alias_contribution(&self) -> String {
        if is_standard_port(self.port) {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

fn is_standard_port(port: u16) -> bool {
    port == 80 || port == 443
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alias_standard_port_omitted() {
        let ep = Endpoint {
            scheme: Scheme::Https,
            host: "api.openai.com".into(),
            port: 443,
        };
        assert_eq!(ep.alias_contribution(), "api.openai.com");
    }

    #[test]
    fn alias_port_80_omitted() {
        let ep = Endpoint {
            scheme: Scheme::Https,
            host: "example.com".into(),
            port: 80,
        };
        assert_eq!(ep.alias_contribution(), "example.com");
    }

    #[test]
    fn alias_nonstandard_port_included() {
        let ep = Endpoint {
            scheme: Scheme::Https,
            host: "api.openai.com".into(),
            port: 8443,
        };
        assert_eq!(ep.alias_contribution(), "api.openai.com:8443");
    }

    #[test]
    fn default_scheme_is_https() {
        let json = r#"{"host": "example.com"}"#;
        let ep: Endpoint = serde_json::from_str(json).unwrap();
        assert_eq!(ep.scheme, Scheme::Https);
        assert_eq!(ep.port, 443);
    }

    #[test]
    fn serde_round_trip() {
        let ep = Endpoint {
            scheme: Scheme::Wss,
            host: "stream.example.com".into(),
            port: 9090,
        };
        let json = serde_json::to_string(&ep).unwrap();
        let ep2: Endpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(ep, ep2);
    }
}
