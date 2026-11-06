use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::PgExecutor;
use utoipa::ToSchema;

use crate::AppState;
use crate::access::OrgCaller;
use crate::error::{ApiError, ErrorBody, JsonBody};

/// 00003 D1's default lateness tolerance, for an organization that has not
/// changed it.
const DEFAULT_TOLERANCE_SECONDS: i32 = 1800;

#[derive(Serialize, ToSchema)]
pub struct OrganizationSettings {
    /// How late, in seconds, a new publication's provider commit may start
    /// after its planned time, when its create request names no tolerance.
    /// It is 1,800 until the organization changes it.
    #[schema(minimum = 300, maximum = 86400)]
    tolerance_seconds: i32,
}

// A field the edit does not define would otherwise be dropped without changing
// anything.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OrganizationSettingsEdit {
    /// How late, in seconds, a new publication's provider commit may start
    /// after its planned time: 300 to 86,400. An organization starts at 1,800.
    /// Existing publications keep the tolerance they have.
    #[schema(minimum = 300, maximum = 86400)]
    tolerance_seconds: i32,
}

/// Refuses a tolerance outside 00003 D1's range.
// The checks on both tables and the five `#[schema(minimum, maximum)]`
// attributes repeat the range. utoipa takes only literals there, so change all
// of them together.
pub fn checked_tolerance(tolerance_seconds: i32) -> Result<i32, ApiError> {
    if !(300..=86_400).contains(&tolerance_seconds) {
        return Err(ApiError::InvalidBody(StatusCode::UNPROCESSABLE_ENTITY));
    }
    Ok(tolerance_seconds)
}

pub async fn default_tolerance(
    db: impl PgExecutor<'_>,
    organization_id: &str,
) -> Result<i32, sqlx::Error> {
    let stored = sqlx::query_scalar(
        "SELECT tolerance_seconds FROM pipewhere.organization_setting WHERE organization_id = $1",
    )
    .bind(organization_id)
    .fetch_optional(db)
    .await?;
    Ok(stored.unwrap_or(DEFAULT_TOLERANCE_SECONDS))
}

#[utoipa::path(
    get,
    path = "/v1/orgs/{organization_id}/settings",
    params(("organization_id" = String, Path)),
    security(("bearer" = [])),
    responses(
        (status = 200, body = OrganizationSettings, description = "The organization's settings"),
        (status = 401, body = ErrorBody, description = "No valid access token"),
        (status = 404, body = ErrorBody, description = "No such organization, or the caller is not in it"),
        (status = 500, body = ErrorBody, description = "A database query failed"),
        (status = 503, body = ErrorBody, description = "Auth's signing keys could not be fetched"),
    )
)]
pub async fn get_settings(
    State(state): State<AppState>,
    caller: OrgCaller,
) -> Result<Json<OrganizationSettings>, ApiError> {
    Ok(Json(OrganizationSettings {
        tolerance_seconds: default_tolerance(&state.db, &caller.organization_id).await?,
    }))
}

#[utoipa::path(
    patch,
    path = "/v1/orgs/{organization_id}/settings",
    params(("organization_id" = String, Path)),
    request_body = OrganizationSettingsEdit,
    security(("bearer" = [])),
    responses(
        (status = 200, body = OrganizationSettings, description = "The edited settings"),
        (status = 400, body = ErrorBody, description = "The request body is not JSON"),
        (status = 401, body = ErrorBody, description = "No valid access token"),
        (status = 404, body = ErrorBody, description = "No such organization, or the caller is not in it"),
        (status = 413, body = ErrorBody, description = "The request body is over 2 MB"),
        (status = 415, body = ErrorBody, description = "The request body is not sent as application/json"),
        (status = 422, body = ErrorBody, description = "The request body is not a valid edit"),
        (status = 500, body = ErrorBody, description = "A database query failed"),
        (status = 503, body = ErrorBody, description = "Auth's signing keys could not be fetched"),
    )
)]
pub async fn update_settings(
    State(state): State<AppState>,
    caller: OrgCaller,
    JsonBody(input): JsonBody<OrganizationSettingsEdit>,
) -> Result<Json<OrganizationSettings>, ApiError> {
    let tolerance_seconds = sqlx::query_scalar(
        "INSERT INTO pipewhere.organization_setting (organization_id, tolerance_seconds)
         VALUES ($1, $2)
         ON CONFLICT (organization_id) DO UPDATE SET tolerance_seconds = EXCLUDED.tolerance_seconds
         RETURNING tolerance_seconds",
    )
    .bind(&caller.organization_id)
    .bind(checked_tolerance(input.tolerance_seconds)?)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(OrganizationSettings { tolerance_seconds }))
}
