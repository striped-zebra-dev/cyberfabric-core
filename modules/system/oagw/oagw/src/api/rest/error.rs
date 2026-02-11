use axum::response::{IntoResponse, Response};
use http::StatusCode;
use oagw_sdk::error::OagwError;

/// Convert an `OagwError` into an axum `Response` with RFC 9457 Problem Details.
pub fn error_response(err: OagwError) -> Response {
    let pd = err.to_problem_details();
    let status = StatusCode::from_u16(pd.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body = serde_json::to_string(&pd).unwrap_or_default();

    let mut response = (
        status,
        [(http::header::CONTENT_TYPE, "application/problem+json")],
        body,
    )
        .into_response();

    response
        .headers_mut()
        .insert("x-oagw-error-source", "gateway".parse().unwrap());

    // Add Retry-After header for 429 responses.
    if let OagwError::RateLimitExceeded {
        retry_after_secs: Some(secs),
        ..
    } = &err
        && let Ok(v) = secs.to_string().parse()
    {
        response.headers_mut().insert("retry-after", v);
    }

    response
}
