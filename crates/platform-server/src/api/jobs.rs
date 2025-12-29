//! Jobs API - Evaluation job queue management
//!
//! Validators claim jobs, report progress, and submit results.

use crate::db::queries;
use crate::models::*;
use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, warn};

// ============================================================================
// REQUEST/RESPONSE TYPES
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct ClaimJobRequest {
    pub validator_hotkey: String,
    pub signature: String,
    pub challenge_id: String,
}

#[derive(Debug, Serialize)]
pub struct ClaimJobResponse {
    pub success: bool,
    pub job: Option<EvaluationJob>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReportProgressRequest {
    pub validator_hotkey: String,
    pub signature: String,
    pub task_index: u32,
    pub task_total: u32,
    pub task_id: String,
    pub status: TaskProgressStatus,
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CompleteJobRequest {
    pub validator_hotkey: String,
    pub signature: String,
    pub score: f64,
    pub tasks_passed: u32,
    pub tasks_total: u32,
    pub total_cost_usd: f64,
    pub execution_time_ms: u64,
    pub task_results: Vec<TaskResultSummary>,
    pub execution_log: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CompleteJobResponse {
    pub success: bool,
    pub evaluation_id: Option<String>,
    pub error: Option<String>,
}

// ============================================================================
// HANDLERS
// ============================================================================

/// POST /api/v1/jobs/claim - Validator claims a pending job
pub async fn claim_job(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ClaimJobRequest>,
) -> Result<Json<ClaimJobResponse>, (StatusCode, Json<ClaimJobResponse>)> {
    // Verify validator signature
    let message = format!("claim_job:{}", req.challenge_id);
    if !crate::api::auth::verify_signature(&req.validator_hotkey, &message, &req.signature) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ClaimJobResponse {
                success: false,
                job: None,
                error: Some("Invalid signature".to_string()),
            }),
        ));
    }

    // Get next pending job for this challenge
    let job =
        match queries::claim_next_job(&state.db, &req.validator_hotkey, &req.challenge_id).await {
            Ok(Some(job)) => job,
            Ok(None) => {
                return Ok(Json(ClaimJobResponse {
                    success: true,
                    job: None,
                    error: None,
                }));
            }
            Err(e) => {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ClaimJobResponse {
                        success: false,
                        job: None,
                        error: Some(e.to_string()),
                    }),
                ));
            }
        };

    info!(
        "Job {} assigned to validator {} (agent: {})",
        job.id,
        req.validator_hotkey,
        &job.agent_hash[..16]
    );

    // Broadcast job assignment
    state
        .broadcast_event(WsEvent::JobAssigned(JobAssignedEvent {
            job_id: job.id.clone(),
            submission_id: job.submission_id.clone(),
            agent_hash: job.agent_hash.clone(),
            validator_hotkey: req.validator_hotkey.clone(),
            challenge_id: req.challenge_id.clone(),
        }))
        .await;

    Ok(Json(ClaimJobResponse {
        success: true,
        job: Some(job),
        error: None,
    }))
}

/// POST /api/v1/jobs/:job_id/progress - Validator reports task progress
pub async fn report_progress(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
    Json(req): Json<ReportProgressRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    // Verify signature
    let message = format!("progress:{}:{}", job_id, req.task_index);
    if !crate::api::auth::verify_signature(&req.validator_hotkey, &message, &req.signature) {
        return Err((StatusCode::UNAUTHORIZED, "Invalid signature".to_string()));
    }

    // Update job status in DB
    queries::update_job_progress(&state.db, &job_id, req.task_index, &req.status.to_string())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Broadcast progress event
    state
        .broadcast_event(WsEvent::JobProgress(JobProgressEvent {
            job_id: job_id.clone(),
            validator_hotkey: req.validator_hotkey.clone(),
            task_index: req.task_index,
            task_total: req.task_total,
            task_id: req.task_id.clone(),
            status: req.status.clone(),
            message: req.message.clone(),
        }))
        .await;

    Ok(Json(serde_json::json!({ "success": true })))
}

/// POST /api/v1/jobs/:job_id/complete - Validator completes job with results
pub async fn complete_job(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
    Json(req): Json<CompleteJobRequest>,
) -> Result<Json<CompleteJobResponse>, (StatusCode, Json<CompleteJobResponse>)> {
    // Verify signature
    let message = format!("complete:{}:{}", job_id, req.score);
    if !crate::api::auth::verify_signature(&req.validator_hotkey, &message, &req.signature) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(CompleteJobResponse {
                success: false,
                evaluation_id: None,
                error: Some("Invalid signature".to_string()),
            }),
        ));
    }

    // Get job details
    let job = queries::get_job(&state.db, &job_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(CompleteJobResponse {
                    success: false,
                    evaluation_id: None,
                    error: Some(e.to_string()),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(CompleteJobResponse {
                    success: false,
                    evaluation_id: None,
                    error: Some("Job not found".to_string()),
                }),
            )
        })?;

    // Save evaluation result
    let evaluation_id = queries::save_evaluation(
        &state.db,
        &job.submission_id,
        &job.agent_hash,
        &req.validator_hotkey,
        req.score,
        req.tasks_passed as i32,
        req.tasks_total as i32,
        req.total_cost_usd,
        req.execution_time_ms as i64,
        &req.task_results,
        req.execution_log.as_deref(),
    )
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(CompleteJobResponse {
                success: false,
                evaluation_id: None,
                error: Some(e.to_string()),
            }),
        )
    })?;

    // Mark job as completed
    queries::complete_job(&state.db, &job_id)
        .await
        .map_err(|e| {
            warn!("Failed to mark job as completed: {}", e);
        })
        .ok();

    info!(
        "Job {} completed by {} - score: {:.2}% ({}/{})",
        job_id,
        req.validator_hotkey,
        req.score * 100.0,
        req.tasks_passed,
        req.tasks_total
    );

    // Broadcast completion event
    state
        .broadcast_event(WsEvent::JobCompleted(JobCompletedEvent {
            job_id: job_id.clone(),
            validator_hotkey: req.validator_hotkey.clone(),
            submission_id: job.submission_id.clone(),
            agent_hash: job.agent_hash.clone(),
            score: req.score,
            tasks_passed: req.tasks_passed,
            tasks_total: req.tasks_total,
            total_cost_usd: req.total_cost_usd,
            execution_time_ms: req.execution_time_ms,
            task_results: req.task_results.clone(),
        }))
        .await;

    // Broadcast evaluation event
    state
        .broadcast_event(WsEvent::EvaluationComplete(EvaluationEvent {
            submission_id: job.submission_id.clone(),
            agent_hash: job.agent_hash.clone(),
            validator_hotkey: req.validator_hotkey.clone(),
            score: req.score,
            tasks_passed: req.tasks_passed,
            tasks_total: req.tasks_total,
        }))
        .await;

    // Check if we have enough evaluations for consensus
    check_and_update_consensus(&state, &job.agent_hash).await;

    Ok(Json(CompleteJobResponse {
        success: true,
        evaluation_id: Some(evaluation_id),
        error: None,
    }))
}

/// Check if we have enough evaluations and update consensus score
async fn check_and_update_consensus(state: &AppState, agent_hash: &str) {
    const MIN_EVALUATIONS_FOR_CONSENSUS: usize = 3;

    match queries::get_evaluations_with_stake(&state.db, agent_hash).await {
        Ok(evaluations) if evaluations.len() >= MIN_EVALUATIONS_FOR_CONSENSUS => {
            // Calculate stake-weighted consensus score
            let total_stake: u64 = evaluations.iter().map(|e| e.validator_stake).sum();
            if total_stake == 0 {
                return;
            }

            let consensus_score: f64 = evaluations
                .iter()
                .map(|e| e.score * (e.validator_stake as f64 / total_stake as f64))
                .sum();

            // Update leaderboard
            if let Err(e) = queries::update_leaderboard(&state.db, agent_hash).await {
                warn!("Failed to update leaderboard: {}", e);
            }

            info!(
                "Consensus reached for {} with {} evaluations: {:.2}%",
                &agent_hash[..16],
                evaluations.len(),
                consensus_score * 100.0
            );
        }
        _ => {}
    }
}

impl ToString for TaskProgressStatus {
    fn to_string(&self) -> String {
        match self {
            TaskProgressStatus::Started => "started".to_string(),
            TaskProgressStatus::Running => "running".to_string(),
            TaskProgressStatus::Passed => "passed".to_string(),
            TaskProgressStatus::Failed => "failed".to_string(),
            TaskProgressStatus::Skipped => "skipped".to_string(),
        }
    }
}
