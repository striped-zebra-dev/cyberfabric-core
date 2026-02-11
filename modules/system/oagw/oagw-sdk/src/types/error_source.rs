//! Error source tracking for OAGW responses

/// Indicates the source of an error in the request/response chain
///
/// Parsed from the `X-OAGW-Error-Source` header to help distinguish between
/// gateway errors (OAGW configuration, rate limits) and upstream errors
/// (OpenAI API errors, service unavailable).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorSource {
    /// Error originated from OAGW gateway
    ///
    /// Examples:
    /// - Invalid alias configuration
    /// - OAGW rate limit exceeded
    /// - Authentication failure with OAGW
    Gateway,

    /// Error originated from upstream service
    ///
    /// Examples:
    /// - OpenAI API rate limit
    /// - Invalid API key for external service
    /// - 404 from upstream service
    Upstream,

    /// Error source unknown
    ///
    /// The `X-OAGW-Error-Source` header was missing or had an unrecognized value
    Unknown,
}

impl ErrorSource {
    /// Parse error source from header value
    ///
    /// # Arguments
    /// * `value` - Header value string (e.g., "gateway", "upstream")
    ///
    /// # Returns
    /// Corresponding `ErrorSource` variant, or `Unknown` if unrecognized
    #[must_use]
    pub fn from_header(value: &str) -> Self {
        match value {
            "gateway" => Self::Gateway,
            "upstream" => Self::Upstream,
            _ => Self::Unknown,
        }
    }

    /// Check if error is from gateway
    #[must_use]
    pub const fn is_gateway(&self) -> bool {
        matches!(self, Self::Gateway)
    }

    /// Check if error is from upstream service
    #[must_use]
    pub const fn is_upstream(&self) -> bool {
        matches!(self, Self::Upstream)
    }

    /// Check if error source is unknown
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }
}

impl Default for ErrorSource {
    fn default() -> Self {
        Self::Unknown
    }
}
