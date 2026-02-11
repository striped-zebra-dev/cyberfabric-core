use uuid::Uuid;

// ---------------------------------------------------------------------------
// GTS identifier schemas
// ---------------------------------------------------------------------------

pub const UPSTREAM_SCHEMA: &str = "gts.x.core.oagw.upstream.v1";
pub const ROUTE_SCHEMA: &str = "gts.x.core.oagw.route.v1";

// ---------------------------------------------------------------------------
// Parse / format
// ---------------------------------------------------------------------------

/// A parsed GTS identifier split at the `~` separator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GtsId {
    pub schema: String,
    pub instance: String,
}

/// Errors returned when parsing a GTS identifier string.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GtsParseError {
    #[error("missing '~' separator")]
    MissingTilde,
    #[error("empty schema or instance")]
    Empty,
    #[error("identifier must start with 'gts.'")]
    InvalidPrefix,
    #[error("invalid UUID in instance: {0}")]
    InvalidUuid(String),
}

impl GtsId {
    /// Parse a GTS identifier string into schema + instance.
    ///
    /// # Errors
    /// Returns `GtsParseError` if the format is invalid.
    pub fn parse(s: &str) -> Result<Self, GtsParseError> {
        let tilde_pos = s.rfind('~').ok_or(GtsParseError::MissingTilde)?;
        let schema = &s[..tilde_pos];
        let instance = &s[tilde_pos + 1..];
        if schema.is_empty() || instance.is_empty() {
            return Err(GtsParseError::Empty);
        }
        if !schema.starts_with("gts.") {
            return Err(GtsParseError::InvalidPrefix);
        }
        Ok(Self {
            schema: schema.to_string(),
            instance: instance.to_string(),
        })
    }

    /// Format a GTS identifier from schema + instance.
    #[must_use]
    pub fn format(schema: &str, instance: &str) -> String {
        format!("{schema}~{instance}")
    }
}

/// Parse a resource GTS identifier (where instance is a UUID).
///
/// # Errors
/// Returns `GtsParseError` if the format is invalid or the instance is not a valid UUID.
pub fn parse_resource_gts(s: &str) -> Result<(String, Uuid), GtsParseError> {
    let gts = GtsId::parse(s)?;
    let uuid =
        Uuid::parse_str(&gts.instance).map_err(|e| GtsParseError::InvalidUuid(e.to_string()))?;
    Ok((gts.schema, uuid))
}

/// Format an upstream resource as a GTS identifier.
#[must_use]
pub fn format_upstream_gts(id: Uuid) -> String {
    GtsId::format(UPSTREAM_SCHEMA, &id.to_string())
}

/// Format a route resource as a GTS identifier.
#[must_use]
pub fn format_route_gts(id: Uuid) -> String {
    GtsId::format(ROUTE_SCHEMA, &id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_upstream_gts() {
        let s = "gts.x.core.oagw.upstream.v1~7c9e6679-7425-40de-944b-e07fc1f90ae7";
        let (schema, uuid) = parse_resource_gts(s).unwrap();
        assert_eq!(schema, "gts.x.core.oagw.upstream.v1");
        assert_eq!(
            uuid,
            Uuid::parse_str("7c9e6679-7425-40de-944b-e07fc1f90ae7").unwrap()
        );
    }

    #[test]
    fn parse_route_gts() {
        let s = "gts.x.core.oagw.route.v1~550e8400-e29b-41d4-a716-446655440000";
        let (schema, uuid) = parse_resource_gts(s).unwrap();
        assert_eq!(schema, "gts.x.core.oagw.route.v1");
        assert_eq!(
            uuid,
            Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap()
        );
    }

    #[test]
    fn format_upstream_round_trip() {
        let id = Uuid::parse_str("7c9e6679-7425-40de-944b-e07fc1f90ae7").unwrap();
        let s = format_upstream_gts(id);
        assert_eq!(
            s,
            "gts.x.core.oagw.upstream.v1~7c9e6679-7425-40de-944b-e07fc1f90ae7"
        );
        let (_, parsed_id) = parse_resource_gts(&s).unwrap();
        assert_eq!(parsed_id, id);
    }

    #[test]
    fn format_route_round_trip() {
        let id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let s = format_route_gts(id);
        let (_, parsed_id) = parse_resource_gts(&s).unwrap();
        assert_eq!(parsed_id, id);
    }

    #[test]
    fn parse_plugin_gts() {
        let s = "gts.x.core.oagw.plugin.auth.v1~x.core.oagw.apikey.v1";
        let gts = GtsId::parse(s).unwrap();
        assert_eq!(gts.schema, "gts.x.core.oagw.plugin.auth.v1");
        assert_eq!(gts.instance, "x.core.oagw.apikey.v1");
    }

    #[test]
    fn reject_missing_tilde() {
        assert_eq!(
            GtsId::parse("not-a-gts-id").unwrap_err(),
            GtsParseError::MissingTilde,
        );
    }

    #[test]
    fn reject_invalid_prefix() {
        assert_eq!(
            GtsId::parse("bad.prefix~uuid").unwrap_err(),
            GtsParseError::InvalidPrefix,
        );
    }

    #[test]
    fn reject_empty_parts() {
        assert_eq!(
            GtsId::parse("gts.something~").unwrap_err(),
            GtsParseError::Empty,
        );
    }

    #[test]
    fn reject_invalid_uuid_in_resource() {
        let s = "gts.x.core.oagw.upstream.v1~not-a-uuid";
        assert!(matches!(
            parse_resource_gts(s).unwrap_err(),
            GtsParseError::InvalidUuid(_),
        ));
    }
}
