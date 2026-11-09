use axum::Json;
use axum::extract::State;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use utoipa::ToSchema;

use crate::AppState;
use crate::access::OrgCaller;
use crate::error::{ApiError, ErrorBody};

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct Scheduler {
    /// The installation's last tick whose query and every send succeeded.
    /// Null until the poller completes its first tick.
    #[schema(required = true)]
    last_completed_at: Option<DateTime<Utc>>,
    /// No tick has completed within the last 60 seconds. This does not check
    /// whether Restate can execute the work it accepted.
    stalled: bool,
}

/// Called after the poller's query and every send succeed, including no sends.
pub async fn record_tick(db: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO pipewhere.scheduler_tick (singleton, last_completed_at)
         VALUES (true, clock_timestamp())
         ON CONFLICT (singleton) DO UPDATE SET last_completed_at = EXCLUDED.last_completed_at",
    )
    .execute(db)
    .await?;
    Ok(())
}

#[utoipa::path(
    get,
    path = "/v1/orgs/{organization_id}/scheduler",
    params(("organization_id" = String, Path)),
    security(("bearer" = [])),
    responses(
        (status = 200, body = Scheduler, description = "The installation's last completed tick and stall status"),
        (status = 401, body = ErrorBody, description = "No valid access token"),
        (status = 404, body = ErrorBody, description = "No such organization, or the caller is not in it"),
        (status = 500, body = ErrorBody, description = "A database query failed"),
        (status = 503, body = ErrorBody, description = "Auth's signing keys could not be fetched"),
    )
)]
pub async fn get_scheduler(
    State(state): State<AppState>,
    _caller: OrgCaller,
) -> Result<Json<Scheduler>, ApiError> {
    let scheduler = sqlx::query_as(
        "SELECT last_completed_at,
                last_completed_at <= clock_timestamp() - interval '60 seconds' AS stalled
         FROM pipewhere.scheduler_tick WHERE singleton",
    )
    .fetch_optional(&state.db)
    .await?
    .unwrap_or(Scheduler {
        last_completed_at: None,
        stalled: true,
    });
    Ok(Json(scheduler))
}
