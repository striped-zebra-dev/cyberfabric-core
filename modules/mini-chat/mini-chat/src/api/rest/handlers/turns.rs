use std::sync::Arc;
use std::time::Duration;

use axum::extract::Path;
use axum::response::sse::KeepAlive;
use axum::response::{IntoResponse, Response, Sse};
use axum::{Extension, Json};
use modkit::api::prelude::*;
use modkit_security::SecurityContext;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use utoipa::ToSchema;

use super::messages::SseRelay;
use crate::domain::repos::TurnRepository;
use crate::domain::service::{MutationError, StreamError};
use crate::domain::stream_events::StreamEvent;
use crate::infra::db::entity::chat_turn::TurnState;
use crate::module::AppServices;

// ════════════════════════════════════════════════════════════════════════════
// GET turn status
// ════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct TurnStatusResponse {
    request_id: uuid::Uuid,
    state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    assistant_message_id: Option<uuid::Uuid>,
    #[serde(with = "time::serde::rfc3339")]
    updated_at: time::OffsetDateTime,
}

fn map_turn_state(state: &TurnState) -> &'static str {
    match state {
        TurnState::Running => "running",
        TurnState::Completed => "done",
        TurnState::Failed => "error",
        TurnState::Cancelled => "cancelled",
    }
}

/// GET /mini-chat/v1/chats/{id}/turns/{request_id}
pub(crate) async fn get_turn(
    Extension(ctx): Extension<SecurityContext>,
    Extension(svc): Extension<Arc<AppServices>>,
    Path((chat_id, request_id)): Path<(uuid::Uuid, uuid::Uuid)>,
) -> ApiResult<Json<TurnStatusResponse>> {
    // Authorization: read_turn scoped to chat
    let scope = svc
        .enforcer
        .access_scope(
            &ctx,
            &crate::domain::service::resources::CHAT,
            crate::domain::service::actions::READ_TURN,
            Some(chat_id),
        )
        .await
        .map_err(|_| Problem::new(StatusCode::NOT_FOUND, "turn_not_found", "Turn not found"))?;
    let scope = scope.tenant_only();

    let conn = svc.db.conn().map_err(|e| {
        Problem::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal Error",
            e.to_string(),
        )
    })?;

    let turn = svc
        .turn_repo
        .find_by_chat_and_request_id(&conn, &scope, chat_id, request_id)
        .await
        .map_err(|e| {
            Problem::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Error",
                e.to_string(),
            )
        })?
        .ok_or_else(|| Problem::new(StatusCode::NOT_FOUND, "turn_not_found", "Turn not found"))?;

    Ok(Json(TurnStatusResponse {
        request_id: turn.request_id,
        state: map_turn_state(&turn.state).to_owned(),
        error_code: turn.error_code.clone(),
        assistant_message_id: turn.assistant_message_id,
        updated_at: turn.updated_at,
    }))
}

// ════════════════════════════════════════════════════════════════════════════
// DELETE turn
// ════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct DeleteTurnResponse {
    request_id: uuid::Uuid,
    deleted: bool,
}

/// DELETE /mini-chat/v1/chats/{id}/turns/{request_id}
pub(crate) async fn delete_turn(
    Extension(ctx): Extension<SecurityContext>,
    Extension(svc): Extension<Arc<AppServices>>,
    Path((chat_id, request_id)): Path<(uuid::Uuid, uuid::Uuid)>,
) -> ApiResult<Json<DeleteTurnResponse>> {
    let result = svc
        .turns
        .delete(&ctx, chat_id, request_id)
        .await
        .map_err(mutation_error_to_problem)?;

    Ok(Json(DeleteTurnResponse {
        request_id: result.request_id,
        deleted: result.deleted,
    }))
}

// ════════════════════════════════════════════════════════════════════════════
// POST retry turn
// ════════════════════════════════════════════════════════════════════════════

/// POST /mini-chat/v1/chats/{id}/turns/{request_id}/retry
pub(crate) async fn retry_turn(
    Extension(ctx): Extension<SecurityContext>,
    Extension(svc): Extension<Arc<AppServices>>,
    Path((chat_id, request_id)): Path<(uuid::Uuid, uuid::Uuid)>,
) -> Response {
    let mutation = match svc.turns.retry(&ctx, chat_id, request_id).await {
        Ok(m) => m,
        Err(e) => return mutation_error_to_problem(e).into_response(),
    };

    start_mutation_stream(&svc, ctx, chat_id, mutation).await
}

// ════════════════════════════════════════════════════════════════════════════
// PATCH edit turn
// ════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize, ToSchema)]
pub struct EditTurnRequest {
    pub content: String,
}

impl modkit::api::api_dto::RequestApiDto for EditTurnRequest {}

/// PATCH /mini-chat/v1/chats/{id}/turns/{request_id}
pub(crate) async fn edit_turn(
    Extension(ctx): Extension<SecurityContext>,
    Extension(svc): Extension<Arc<AppServices>>,
    Path((chat_id, request_id)): Path<(uuid::Uuid, uuid::Uuid)>,
    Json(body): Json<EditTurnRequest>,
) -> Response {
    if body.content.trim().is_empty() {
        return Problem::new(
            StatusCode::BAD_REQUEST,
            "Bad Request",
            "Edit content must not be empty",
        )
        .into_response();
    }

    let mutation = match svc
        .turns
        .edit(&ctx, chat_id, request_id, body.content)
        .await
    {
        Ok(m) => m,
        Err(e) => return mutation_error_to_problem(e).into_response(),
    };

    start_mutation_stream(&svc, ctx, chat_id, mutation).await
}

// ════════════════════════════════════════════════════════════════════════════
// Shared helpers
// ════════════════════════════════════════════════════════════════════════════

async fn start_mutation_stream(
    svc: &AppServices,
    ctx: SecurityContext,
    chat_id: uuid::Uuid,
    mutation: crate::domain::service::MutationResult,
) -> Response {
    let chat = match svc.chats.get_chat(&ctx, chat_id).await {
        Ok(c) => c,
        Err(e) => {
            return Problem::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Error",
                e.to_string(),
            )
            .into_response();
        }
    };

    let resolved = match svc
        .models
        .resolve_model(ctx.subject_id(), Some(chat.model))
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return Problem::new(StatusCode::BAD_REQUEST, "Bad Request", e.to_string())
                .into_response();
        }
    };

    let capacity = svc.stream.channel_capacity();
    let ping_secs = svc.stream.ping_interval_secs();
    let (tx, rx) = mpsc::channel::<StreamEvent>(capacity);
    let cancel = CancellationToken::new();

    info!(
        chat_id = %chat_id,
        new_request_id = %mutation.new_request_id,
        model = %resolved.model_id,
        "starting mutation SSE stream"
    );

    let provider_handle = match svc
        .stream
        .run_stream_for_mutation(
            ctx,
            chat_id,
            mutation.new_request_id,
            mutation.new_turn_id,
            mutation.user_content,
            resolved,
            cancel.clone(),
            tx,
        )
        .await
    {
        Ok(handle) => handle,
        Err(e) => return stream_error_to_response(e),
    };

    tokio::spawn(async move {
        if let Err(e) = provider_handle.await {
            tracing::error!(error = ?e, "provider task panicked");
        }
    });

    let relay = SseRelay::new(rx, cancel, ping_secs);
    Sse::new(relay)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(30)))
        .into_response()
}

/// Map `MutationError` to HTTP problem response.
fn mutation_error_to_problem(err: MutationError) -> Problem {
    match err {
        MutationError::ChatNotFound { .. } => {
            Problem::new(StatusCode::NOT_FOUND, "chat_not_found", "Chat not found")
        }
        MutationError::TurnNotFound { .. } => {
            Problem::new(StatusCode::NOT_FOUND, "turn_not_found", "Turn not found")
        }
        MutationError::InsufficientPermissions => Problem::new(
            StatusCode::FORBIDDEN,
            "insufficient_permissions",
            "You do not have permission to modify this turn",
        ),
        MutationError::InvalidTurnState { state } => Problem::new(
            StatusCode::BAD_REQUEST,
            "invalid_turn_state",
            format!("Turn is in {state:?} state; only terminal turns can be mutated"),
        ),
        MutationError::NotLatestTurn => Problem::new(
            StatusCode::CONFLICT,
            "not_latest_turn",
            "Only the most recent turn can be mutated",
        ),
        MutationError::GenerationInProgress => Problem::new(
            StatusCode::CONFLICT,
            "generation_in_progress",
            "Another generation is already in progress for this chat",
        ),
        MutationError::Internal { message } => {
            warn!(%message, "turn mutation internal error");
            Problem::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Error",
                "An internal error occurred",
            )
        }
    }
}

fn stream_error_to_response(err: StreamError) -> Response {
    match err {
        StreamError::QuotaExhausted {
            error_code,
            http_status,
        } => {
            let status = StatusCode::from_u16(http_status).unwrap_or(StatusCode::TOO_MANY_REQUESTS);
            Problem::new(status, "Quota Exhausted", &error_code).into_response()
        }
        other => {
            warn!(error = ?other, "post-mutation stream error");
            Problem::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Error",
                "Failed to start streaming",
            )
            .into_response()
        }
    }
}
