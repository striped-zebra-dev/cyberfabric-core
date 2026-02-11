use axum::Json;
use axum::extract::{Extension, Path, Query};
use axum::response::{IntoResponse, Response};
use http::StatusCode;
use oagw_sdk::gts;
use oagw_sdk::models::{CreateUpstreamRequest, UpdateUpstreamRequest, Upstream, UpstreamResponse};

use crate::api::rest::error::error_response;
use crate::api::rest::extractors::{PaginationQuery, TenantId, parse_gts_id};
use crate::module::AppState;

fn to_response(u: Upstream) -> UpstreamResponse {
    UpstreamResponse {
        id: gts::format_upstream_gts(u.id),
        tenant_id: u.tenant_id,
        alias: u.alias,
        server: u.server,
        protocol: u.protocol,
        enabled: u.enabled,
        auth: u.auth,
        headers: u.headers,
        plugins: u.plugins,
        rate_limit: u.rate_limit,
        tags: u.tags,
    }
}

pub async fn create_upstream(
    Extension(state): Extension<AppState>,
    tenant: TenantId,
    Json(req): Json<CreateUpstreamRequest>,
) -> Result<impl IntoResponse, Response> {
    let upstream = state
        .cp
        .create_upstream(tenant.0, req)
        .await
        .map_err(error_response)?;
    Ok((StatusCode::CREATED, Json(to_response(upstream))))
}

pub async fn get_upstream(
    Extension(state): Extension<AppState>,
    tenant: TenantId,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, Response> {
    let uuid = parse_gts_id(&id, &format!("/oagw/v1/upstreams/{id}"))?;
    let upstream = state
        .cp
        .get_upstream(tenant.0, uuid)
        .await
        .map_err(error_response)?;
    Ok(Json(to_response(upstream)))
}

pub async fn list_upstreams(
    Extension(state): Extension<AppState>,
    tenant: TenantId,
    Query(pagination): Query<PaginationQuery>,
) -> Result<impl IntoResponse, Response> {
    let query = pagination.to_list_query();
    let upstreams = state
        .cp
        .list_upstreams(tenant.0, &query)
        .await
        .map_err(error_response)?;
    let response: Vec<UpstreamResponse> = upstreams.into_iter().map(to_response).collect();
    Ok(Json(response))
}

pub async fn update_upstream(
    Extension(state): Extension<AppState>,
    tenant: TenantId,
    Path(id): Path<String>,
    Json(req): Json<UpdateUpstreamRequest>,
) -> Result<impl IntoResponse, Response> {
    let uuid = parse_gts_id(&id, &format!("/oagw/v1/upstreams/{id}"))?;
    let upstream = state
        .cp
        .update_upstream(tenant.0, uuid, req)
        .await
        .map_err(error_response)?;
    Ok(Json(to_response(upstream)))
}

pub async fn delete_upstream(
    Extension(state): Extension<AppState>,
    tenant: TenantId,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, Response> {
    let uuid = parse_gts_id(&id, &format!("/oagw/v1/upstreams/{id}"))?;
    state
        .cp
        .delete_upstream(tenant.0, uuid)
        .await
        .map_err(error_response)?;
    Ok(StatusCode::NO_CONTENT)
}
