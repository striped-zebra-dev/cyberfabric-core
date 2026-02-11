use std::pin::Pin;

use bytes::Bytes;
use futures_core::Stream;
use http::{HeaderMap, Method, StatusCode};
use uuid::Uuid;

use crate::error::OagwError;
use crate::models::ListQuery;
use crate::models::{
    CreateRouteRequest, CreateUpstreamRequest, Route, UpdateRouteRequest, UpdateUpstreamRequest,
    Upstream,
};

// ---------------------------------------------------------------------------
// Body / Error aliases
// ---------------------------------------------------------------------------

/// Boxed error type for body stream errors.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// A streaming response body.
pub type BodyStream = Pin<Box<dyn Stream<Item = Result<Bytes, BoxError>> + Send>>;

// ---------------------------------------------------------------------------
// Proxy types
// ---------------------------------------------------------------------------

/// Distinguishes gateway-originated errors from upstream-originated errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorSource {
    Gateway,
    Upstream,
}

/// Context for a proxy request flowing through the Data Plane.
pub struct ProxyContext {
    pub tenant_id: Uuid,
    pub method: Method,
    pub alias: String,
    pub path_suffix: String,
    pub query_params: Vec<(String, String)>,
    pub headers: HeaderMap,
    /// Request body (already validated for size).
    pub body: Bytes,
    /// Original request URI for error reporting.
    pub instance_uri: String,
}

/// Response from the Data Plane proxy pipeline.
pub struct ProxyResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: BodyStream,
    pub error_source: ErrorSource,
}

// ---------------------------------------------------------------------------
// Service traits
// ---------------------------------------------------------------------------

/// Control Plane service — configuration management and resolution.
#[async_trait::async_trait]
pub trait ControlPlaneService: Send + Sync {
    // -- Upstream CRUD --

    async fn create_upstream(
        &self,
        tenant_id: Uuid,
        req: CreateUpstreamRequest,
    ) -> Result<Upstream, OagwError>;

    async fn get_upstream(&self, tenant_id: Uuid, id: Uuid) -> Result<Upstream, OagwError>;

    async fn list_upstreams(
        &self,
        tenant_id: Uuid,
        query: &ListQuery,
    ) -> Result<Vec<Upstream>, OagwError>;

    async fn update_upstream(
        &self,
        tenant_id: Uuid,
        id: Uuid,
        req: UpdateUpstreamRequest,
    ) -> Result<Upstream, OagwError>;

    async fn delete_upstream(&self, tenant_id: Uuid, id: Uuid) -> Result<(), OagwError>;

    // -- Route CRUD --

    async fn create_route(
        &self,
        tenant_id: Uuid,
        req: CreateRouteRequest,
    ) -> Result<Route, OagwError>;

    async fn get_route(&self, tenant_id: Uuid, id: Uuid) -> Result<Route, OagwError>;

    async fn list_routes(
        &self,
        tenant_id: Uuid,
        upstream_id: Uuid,
        query: &ListQuery,
    ) -> Result<Vec<Route>, OagwError>;

    async fn update_route(
        &self,
        tenant_id: Uuid,
        id: Uuid,
        req: UpdateRouteRequest,
    ) -> Result<Route, OagwError>;

    async fn delete_route(&self, tenant_id: Uuid, id: Uuid) -> Result<(), OagwError>;

    // -- Resolution --

    /// Resolve an upstream by alias. Returns UpstreamDisabled if the upstream exists but is disabled.
    async fn resolve_upstream(&self, tenant_id: Uuid, alias: &str) -> Result<Upstream, OagwError>;

    /// Find the best matching route for the given method and path under an upstream.
    async fn resolve_route(
        &self,
        tenant_id: Uuid,
        upstream_id: Uuid,
        method: &str,
        path: &str,
    ) -> Result<Route, OagwError>;
}

/// Data Plane service — proxy orchestration and plugin execution.
#[async_trait::async_trait]
pub trait DataPlaneService: Send + Sync {
    /// Execute the full proxy pipeline: resolve → auth → rate-limit → forward → respond.
    async fn proxy_request(&self, ctx: ProxyContext) -> Result<ProxyResponse, OagwError>;
}
