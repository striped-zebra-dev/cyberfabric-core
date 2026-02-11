pub mod config;
pub mod dto;
pub mod endpoint;
pub mod route;
pub mod upstream;

// Re-export commonly used types at the models level.
pub use config::{
    AuthConfig, BurstConfig, HeadersConfig, PassthroughMode, PluginsConfig, RateLimitAlgorithm,
    RateLimitConfig, RateLimitScope, RateLimitStrategy, RequestHeaderRules, ResponseHeaderRules,
    SharingMode, SustainedRate, Window,
};
pub use dto::{
    CreateRouteRequest, CreateUpstreamRequest, RouteResponse, UpdateRouteRequest,
    UpdateUpstreamRequest, UpstreamResponse,
};
pub use endpoint::{Endpoint, Scheme};
pub use route::{GrpcMatch, HttpMatch, HttpMethod, MatchRules, PathSuffixMode, Route};
pub use upstream::{Server, Upstream};

// ---------------------------------------------------------------------------
// Pagination
// ---------------------------------------------------------------------------

/// Pagination parameters for list queries.
#[derive(Debug, Clone)]
pub struct ListQuery {
    /// Maximum number of items to return.
    pub top: u32,
    /// Number of items to skip.
    pub skip: u32,
}

impl Default for ListQuery {
    fn default() -> Self {
        Self { top: 50, skip: 0 }
    }
}
