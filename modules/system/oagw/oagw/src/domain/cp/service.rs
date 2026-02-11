use std::sync::Arc;

use super::repo::{RepositoryError, RouteRepository, UpstreamRepository};
use modkit_macros::domain_model;
use oagw_sdk::error::OagwError;
use oagw_sdk::models::ListQuery;
use oagw_sdk::models::{
    CreateRouteRequest, CreateUpstreamRequest, Route, UpdateRouteRequest, UpdateUpstreamRequest,
    Upstream,
};
use oagw_sdk::service::ControlPlaneService;
use uuid::Uuid;

/// Control Plane service implementation backed by in-memory repositories.
#[domain_model]
pub(crate) struct ControlPlaneServiceImpl {
    upstreams: Arc<dyn UpstreamRepository>,
    routes: Arc<dyn RouteRepository>,
}

impl ControlPlaneServiceImpl {
    #[must_use]
    pub(crate) fn new(
        upstreams: Arc<dyn UpstreamRepository>,
        routes: Arc<dyn RouteRepository>,
    ) -> Self {
        Self { upstreams, routes }
    }
}

/// Generate an alias from the upstream's server endpoints.
/// Single endpoint: host (standard port omitted) or host:port.
fn generate_alias(upstream: &Upstream) -> String {
    let endpoints = &upstream.server.endpoints;
    if endpoints.is_empty() {
        return String::new();
    }
    // Use the first endpoint for alias generation.
    endpoints[0].alias_contribution()
}

fn map_repo_error(err: RepositoryError, instance: &str) -> OagwError {
    match err {
        RepositoryError::NotFound => OagwError::RouteNotFound {
            instance: instance.to_string(),
        },
        RepositoryError::Conflict(detail) => OagwError::ValidationError {
            detail,
            instance: instance.to_string(),
        },
        RepositoryError::Internal(detail) => OagwError::DownstreamError {
            detail,
            instance: instance.to_string(),
        },
    }
}

#[async_trait::async_trait]
impl ControlPlaneService for ControlPlaneServiceImpl {
    // -- Upstream CRUD --

    async fn create_upstream(
        &self,
        tenant_id: Uuid,
        req: CreateUpstreamRequest,
    ) -> Result<Upstream, OagwError> {
        let id = Uuid::new_v4();
        let alias = req.alias.unwrap_or_else(|| {
            let temp = Upstream {
                id,
                tenant_id,
                alias: String::new(),
                server: req.server.clone(),
                protocol: req.protocol.clone(),
                enabled: req.enabled,
                auth: req.auth.clone(),
                headers: req.headers.clone(),
                plugins: req.plugins.clone(),
                rate_limit: req.rate_limit.clone(),
                tags: req.tags.clone(),
            };
            generate_alias(&temp)
        });

        let upstream = Upstream {
            id,
            tenant_id,
            alias,
            server: req.server,
            protocol: req.protocol,
            enabled: req.enabled,
            auth: req.auth,
            headers: req.headers,
            plugins: req.plugins,
            rate_limit: req.rate_limit,
            tags: req.tags,
        };

        self.upstreams
            .create(upstream)
            .await
            .map_err(|e| map_repo_error(e, "/oagw/v1/upstreams"))
    }

    async fn get_upstream(&self, tenant_id: Uuid, id: Uuid) -> Result<Upstream, OagwError> {
        self.upstreams
            .get_by_id(tenant_id, id)
            .await
            .map_err(|e| map_repo_error(e, &format!("/oagw/v1/upstreams/{id}")))
    }

    async fn list_upstreams(
        &self,
        tenant_id: Uuid,
        query: &ListQuery,
    ) -> Result<Vec<Upstream>, OagwError> {
        self.upstreams
            .list(tenant_id, query)
            .await
            .map_err(|e| map_repo_error(e, "/oagw/v1/upstreams"))
    }

    async fn update_upstream(
        &self,
        tenant_id: Uuid,
        id: Uuid,
        req: UpdateUpstreamRequest,
    ) -> Result<Upstream, OagwError> {
        let instance = format!("/oagw/v1/upstreams/{id}");
        let mut existing = self
            .upstreams
            .get_by_id(tenant_id, id)
            .await
            .map_err(|e| map_repo_error(e, &instance))?;

        // Apply partial update.
        if let Some(server) = req.server {
            existing.server = server;
        }
        if let Some(protocol) = req.protocol {
            existing.protocol = protocol;
        }
        if let Some(alias) = req.alias {
            existing.alias = alias;
        }
        if let Some(auth) = req.auth {
            existing.auth = Some(auth);
        }
        if let Some(headers) = req.headers {
            existing.headers = Some(headers);
        }
        if let Some(plugins) = req.plugins {
            existing.plugins = Some(plugins);
        }
        if let Some(rate_limit) = req.rate_limit {
            existing.rate_limit = Some(rate_limit);
        }
        if let Some(tags) = req.tags {
            existing.tags = tags;
        }
        if let Some(enabled) = req.enabled {
            existing.enabled = enabled;
        }

        self.upstreams
            .update(existing)
            .await
            .map_err(|e| map_repo_error(e, &instance))
    }

    async fn delete_upstream(&self, tenant_id: Uuid, id: Uuid) -> Result<(), OagwError> {
        let instance = format!("/oagw/v1/upstreams/{id}");
        // Cascade delete routes.
        let _ = self.routes.delete_by_upstream(tenant_id, id).await;
        self.upstreams
            .delete(tenant_id, id)
            .await
            .map_err(|e| map_repo_error(e, &instance))
    }

    // -- Route CRUD --

    async fn create_route(
        &self,
        tenant_id: Uuid,
        req: CreateRouteRequest,
    ) -> Result<Route, OagwError> {
        // Validate that the upstream exists and belongs to this tenant.
        let instance = "/oagw/v1/routes";
        self.upstreams
            .get_by_id(tenant_id, req.upstream_id)
            .await
            .map_err(|_| OagwError::ValidationError {
                detail: format!("upstream '{}' not found for this tenant", req.upstream_id),
                instance: instance.to_string(),
            })?;

        let route = Route {
            id: Uuid::new_v4(),
            tenant_id,
            upstream_id: req.upstream_id,
            match_rules: req.match_rules,
            plugins: req.plugins,
            rate_limit: req.rate_limit,
            tags: req.tags,
            priority: req.priority,
            enabled: req.enabled,
        };

        self.routes
            .create(route)
            .await
            .map_err(|e| map_repo_error(e, instance))
    }

    async fn get_route(&self, tenant_id: Uuid, id: Uuid) -> Result<Route, OagwError> {
        self.routes
            .get_by_id(tenant_id, id)
            .await
            .map_err(|e| map_repo_error(e, &format!("/oagw/v1/routes/{id}")))
    }

    async fn list_routes(
        &self,
        tenant_id: Uuid,
        upstream_id: Uuid,
        query: &ListQuery,
    ) -> Result<Vec<Route>, OagwError> {
        self.routes
            .list_by_upstream(tenant_id, upstream_id, query)
            .await
            .map_err(|e| map_repo_error(e, "/oagw/v1/routes"))
    }

    async fn update_route(
        &self,
        tenant_id: Uuid,
        id: Uuid,
        req: UpdateRouteRequest,
    ) -> Result<Route, OagwError> {
        let instance = format!("/oagw/v1/routes/{id}");
        let mut existing = self
            .routes
            .get_by_id(tenant_id, id)
            .await
            .map_err(|e| map_repo_error(e, &instance))?;

        if let Some(match_rules) = req.match_rules {
            existing.match_rules = match_rules;
        }
        if let Some(plugins) = req.plugins {
            existing.plugins = Some(plugins);
        }
        if let Some(rate_limit) = req.rate_limit {
            existing.rate_limit = Some(rate_limit);
        }
        if let Some(tags) = req.tags {
            existing.tags = tags;
        }
        if let Some(priority) = req.priority {
            existing.priority = priority;
        }
        if let Some(enabled) = req.enabled {
            existing.enabled = enabled;
        }

        self.routes
            .update(existing)
            .await
            .map_err(|e| map_repo_error(e, &instance))
    }

    async fn delete_route(&self, tenant_id: Uuid, id: Uuid) -> Result<(), OagwError> {
        self.routes
            .delete(tenant_id, id)
            .await
            .map_err(|e| map_repo_error(e, &format!("/oagw/v1/routes/{id}")))
    }

    // -- Resolution --

    async fn resolve_upstream(&self, tenant_id: Uuid, alias: &str) -> Result<Upstream, OagwError> {
        let upstream = self
            .upstreams
            .get_by_alias(tenant_id, alias)
            .await
            .map_err(|_| OagwError::RouteNotFound {
                instance: format!("/oagw/v1/proxy/{alias}"),
            })?;

        if !upstream.enabled {
            return Err(OagwError::UpstreamDisabled {
                detail: format!("upstream '{alias}' is disabled"),
                instance: format!("/oagw/v1/proxy/{alias}"),
            });
        }

        Ok(upstream)
    }

    async fn resolve_route(
        &self,
        tenant_id: Uuid,
        upstream_id: Uuid,
        method: &str,
        path: &str,
    ) -> Result<Route, OagwError> {
        self.routes
            .find_matching(tenant_id, upstream_id, method, path)
            .await
            .map_err(|_| OagwError::RouteNotFound {
                instance: format!("/oagw/v1/proxy (method={method}, path={path})"),
            })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use oagw_sdk::models::{
        Endpoint, HttpMatch, HttpMethod, MatchRules, PathSuffixMode, Server, endpoint::Scheme,
    };

    use super::super::repo::{InMemoryRouteRepo, InMemoryUpstreamRepo};
    use super::*;

    fn make_service() -> ControlPlaneServiceImpl {
        ControlPlaneServiceImpl::new(
            Arc::new(InMemoryUpstreamRepo::new()),
            Arc::new(InMemoryRouteRepo::new()),
        )
    }

    fn make_create_upstream(alias: Option<&str>) -> CreateUpstreamRequest {
        CreateUpstreamRequest {
            server: Server {
                endpoints: vec![Endpoint {
                    scheme: Scheme::Https,
                    host: "api.openai.com".into(),
                    port: 443,
                }],
            },
            protocol: "gts.x.core.oagw.protocol.v1~x.core.oagw.http.v1".into(),
            alias: alias.map(String::from),
            auth: None,
            headers: None,
            plugins: None,
            rate_limit: None,
            tags: vec![],
            enabled: true,
        }
    }

    fn make_create_route(upstream_id: Uuid) -> CreateRouteRequest {
        CreateRouteRequest {
            upstream_id,
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

    #[tokio::test]
    async fn upstream_crud_lifecycle() {
        let svc = make_service();
        let tenant = Uuid::new_v4();

        // Create
        let u = svc
            .create_upstream(tenant, make_create_upstream(Some("openai")))
            .await
            .unwrap();
        assert_eq!(u.alias, "openai");

        // Get
        let fetched = svc.get_upstream(tenant, u.id).await.unwrap();
        assert_eq!(fetched.id, u.id);

        // Update
        let updated = svc
            .update_upstream(
                tenant,
                u.id,
                UpdateUpstreamRequest {
                    alias: Some("openai-v2".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.alias, "openai-v2");
        assert_eq!(updated.id, u.id);

        // List
        let list = svc
            .list_upstreams(tenant, &ListQuery::default())
            .await
            .unwrap();
        assert_eq!(list.len(), 1);

        // Delete
        svc.delete_upstream(tenant, u.id).await.unwrap();
        assert!(svc.get_upstream(tenant, u.id).await.is_err());
    }

    #[tokio::test]
    async fn alias_auto_generation() {
        let svc = make_service();
        let tenant = Uuid::new_v4();

        // Standard port (443) — port omitted in alias.
        let u1 = svc
            .create_upstream(tenant, make_create_upstream(None))
            .await
            .unwrap();
        assert_eq!(u1.alias, "api.openai.com");

        // Non-standard port — port included.
        let req = CreateUpstreamRequest {
            server: Server {
                endpoints: vec![Endpoint {
                    scheme: Scheme::Https,
                    host: "api.openai.com".into(),
                    port: 8443,
                }],
            },
            protocol: "gts.x.core.oagw.protocol.v1~x.core.oagw.http.v1".into(),
            alias: None,
            auth: None,
            headers: None,
            plugins: None,
            rate_limit: None,
            tags: vec![],
            enabled: true,
        };
        let u2 = svc.create_upstream(tenant, req).await.unwrap();
        assert_eq!(u2.alias, "api.openai.com:8443");
    }

    #[tokio::test]
    async fn duplicate_alias_conflict() {
        let svc = make_service();
        let tenant = Uuid::new_v4();

        svc.create_upstream(tenant, make_create_upstream(Some("openai")))
            .await
            .unwrap();

        let err = svc
            .create_upstream(tenant, make_create_upstream(Some("openai")))
            .await
            .unwrap_err();
        assert_eq!(err.status(), 400);
    }

    #[tokio::test]
    async fn route_create_with_wrong_tenant_upstream() {
        let svc = make_service();
        let t1 = Uuid::new_v4();
        let t2 = Uuid::new_v4();

        let u = svc
            .create_upstream(t1, make_create_upstream(Some("openai")))
            .await
            .unwrap();

        // Try to create route in different tenant referencing t1's upstream.
        let err = svc
            .create_route(t2, make_create_route(u.id))
            .await
            .unwrap_err();
        assert_eq!(err.status(), 400);
    }

    #[tokio::test]
    async fn alias_resolution_enabled() {
        let svc = make_service();
        let tenant = Uuid::new_v4();

        let u = svc
            .create_upstream(tenant, make_create_upstream(Some("openai")))
            .await
            .unwrap();

        let resolved = svc.resolve_upstream(tenant, "openai").await.unwrap();
        assert_eq!(resolved.id, u.id);
    }

    #[tokio::test]
    async fn alias_resolution_disabled_returns_503() {
        let svc = make_service();
        let tenant = Uuid::new_v4();

        let u = svc
            .create_upstream(tenant, make_create_upstream(Some("openai")))
            .await
            .unwrap();

        // Disable the upstream.
        svc.update_upstream(
            tenant,
            u.id,
            UpdateUpstreamRequest {
                enabled: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let err = svc.resolve_upstream(tenant, "openai").await.unwrap_err();
        assert_eq!(err.status(), 503);
    }

    #[tokio::test]
    async fn alias_resolution_nonexistent_returns_404() {
        let svc = make_service();
        let tenant = Uuid::new_v4();

        let err = svc
            .resolve_upstream(tenant, "nonexistent")
            .await
            .unwrap_err();
        assert_eq!(err.status(), 404);
    }

    #[tokio::test]
    async fn route_matching_through_cp() {
        let svc = make_service();
        let tenant = Uuid::new_v4();

        let u = svc
            .create_upstream(tenant, make_create_upstream(Some("openai")))
            .await
            .unwrap();
        let r = svc
            .create_route(tenant, make_create_route(u.id))
            .await
            .unwrap();

        let matched = svc
            .resolve_route(tenant, u.id, "POST", "/v1/chat/completions")
            .await
            .unwrap();
        assert_eq!(matched.id, r.id);
    }

    #[tokio::test]
    async fn route_matching_no_match_returns_404() {
        let svc = make_service();
        let tenant = Uuid::new_v4();

        let u = svc
            .create_upstream(tenant, make_create_upstream(Some("openai")))
            .await
            .unwrap();

        let err = svc
            .resolve_route(tenant, u.id, "GET", "/v1/unknown")
            .await
            .unwrap_err();
        assert_eq!(err.status(), 404);
    }

    #[tokio::test]
    async fn delete_upstream_cascades_routes() {
        let svc = make_service();
        let tenant = Uuid::new_v4();

        let u = svc
            .create_upstream(tenant, make_create_upstream(Some("openai")))
            .await
            .unwrap();
        let r = svc
            .create_route(tenant, make_create_route(u.id))
            .await
            .unwrap();

        svc.delete_upstream(tenant, u.id).await.unwrap();

        // Route should be gone.
        assert!(svc.get_route(tenant, r.id).await.is_err());
    }
}
