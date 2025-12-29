//! Submissions API handlers - DEPRECATED
//!
//! Submissions have been migrated to term-challenge.
//! These endpoints now return deprecation notices directing users
//! to use the term-challenge API instead.

use crate::db::queries;
use crate::models::*;
use crate::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use std::sync::Arc;

/// Term-challenge URL for submissions (configured via TERM_CHALLENGE_URL env)
fn get_term_challenge_url() -> String {
    std::env::var("TERM_CHALLENGE_URL").unwrap_or_else(|_| "http://localhost:8081".to_string())
}

#[derive(Debug, Deserialize)]
pub struct ListSubmissionsQuery {
    pub limit: Option<usize>,
    pub status: Option<String>,
}

/// DEPRECATED: List submissions
/// Submissions are now managed by term-challenge.
pub async fn list_submissions(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListSubmissionsQuery>,
) -> Result<Json<Vec<Submission>>, StatusCode> {
    // Still return data from local DB for backward compatibility during migration
    let submissions = if query.status.as_deref() == Some("pending") {
        queries::get_pending_submissions(&state.db).await
    } else {
        queries::get_pending_submissions(&state.db).await
    }
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let limit = query.limit.unwrap_or(100);
    let limited: Vec<_> = submissions.into_iter().take(limit).collect();
    Ok(Json(limited))
}

/// DEPRECATED: Get submission by ID
/// Submissions are now managed by term-challenge.
pub async fn get_submission(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Submission>, StatusCode> {
    let submission = queries::get_submission(&state.db, &id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(submission))
}

/// DEPRECATED: Get submission source code
/// Source code access is now managed by term-challenge with proper authentication.
pub async fn get_submission_source(
    State(_state): State<Arc<AppState>>,
    Path(_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // No longer expose source code from platform-server
    // Users must use term-challenge API with proper authentication
    Err((
        StatusCode::GONE,
        Json(serde_json::json!({
            "error": "Source code access has been migrated to term-challenge",
            "message": "Use the term-challenge API to access source code with proper authentication",
            "term_challenge_url": get_term_challenge_url(),
            "endpoints": {
                "owner_source": "POST /api/v1/my/agents/:hash/source",
                "validator_claim": "POST /api/v1/validator/claim_job"
            }
        })),
    ))
}

/// DEPRECATED: Submit agent endpoint
/// Agent submissions have been migrated to term-challenge.
/// This endpoint returns a redirect notice.
pub async fn submit_agent(
    State(_state): State<Arc<AppState>>,
    Json(_req): Json<SubmitAgentRequest>,
) -> Result<Json<SubmitAgentResponse>, (StatusCode, Json<SubmitAgentResponse>)> {
    // Submissions are now handled by term-challenge
    tracing::warn!("Deprecated submit_agent endpoint called - redirecting to term-challenge");

    Err((
        StatusCode::GONE,
        Json(SubmitAgentResponse {
            success: false,
            submission_id: None,
            agent_hash: None,
            error: Some(format!(
                "Agent submissions have been migrated to term-challenge. \
                 Please use: POST {}/api/v1/submit",
                get_term_challenge_url()
            )),
        }),
    ))
}
