use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::AppState;
use crate::access::OrgCaller;
use crate::error::{ApiError, ErrorBody, JsonBody};
use crate::ids::new_id;

/// Private source writing, with no channel or time. Editing it never edits a
/// publication made from it.
#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct Content {
    id: String,
    organization_id: String,
    body: String,
}

#[derive(Deserialize, ToSchema)]
pub struct ContentInput {
    /// The text. It must hold a character other than whitespace.
    body: String,
}

impl ContentInput {
    fn into_body(self) -> Result<String, ApiError> {
        if self.body.trim().is_empty() {
            return Err(ApiError::InvalidBody(StatusCode::UNPROCESSABLE_ENTITY));
        }
        Ok(self.body)
    }
}

#[utoipa::path(
    post,
    path = "/v1/orgs/{organization_id}/contents",
    params(("organization_id" = String, Path)),
    request_body = ContentInput,
    security(("bearer" = [])),
    responses(
        (status = 201, body = Content, description = "The content was created"),
        (status = 400, body = ErrorBody, description = "The request body is not JSON"),
        (status = 401, body = ErrorBody, description = "No valid access token"),
        (status = 404, body = ErrorBody, description = "No such organization, or the caller is not in it"),
        (status = 413, body = ErrorBody, description = "The request body is over 2 MB"),
        (status = 415, body = ErrorBody, description = "The request body is not sent as application/json"),
        (status = 422, body = ErrorBody, description = "The request body is not valid content"),
        (status = 500, body = ErrorBody, description = "A database query failed"),
        (status = 503, body = ErrorBody, description = "Auth's signing keys could not be fetched"),
    )
)]
pub async fn create_content(
    State(state): State<AppState>,
    caller: OrgCaller,
    JsonBody(input): JsonBody<ContentInput>,
) -> Result<(StatusCode, Json<Content>), ApiError> {
    let content = sqlx::query_as(
        "INSERT INTO pipewhere.content (id, organization_id, body) VALUES ($1, $2, $3)
         RETURNING id, organization_id, body",
    )
    .bind(new_id("content"))
    .bind(&caller.organization_id)
    .bind(input.into_body()?)
    .fetch_one(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(content)))
}

#[utoipa::path(
    get,
    path = "/v1/orgs/{organization_id}/contents/{content_id}",
    params(("organization_id" = String, Path), ("content_id" = String, Path)),
    security(("bearer" = [])),
    responses(
        (status = 200, body = Content, description = "The content"),
        (status = 401, body = ErrorBody, description = "No valid access token"),
        (status = 404, body = ErrorBody, description = "No such content in the organization, or the caller is not in it"),
        (status = 500, body = ErrorBody, description = "A database query failed"),
        (status = 503, body = ErrorBody, description = "Auth's signing keys could not be fetched"),
    )
)]
pub async fn get_content(
    State(state): State<AppState>,
    caller: OrgCaller,
    Path((_, content_id)): Path<(String, String)>,
) -> Result<Json<Content>, ApiError> {
    sqlx::query_as(
        "SELECT id, organization_id, body FROM pipewhere.content WHERE id = $1 AND organization_id = $2",
    )
    .bind(&content_id)
    .bind(&caller.organization_id)
    .fetch_optional(&state.db)
    .await?
    .map(Json)
    .ok_or(ApiError::NotFound)
}

#[utoipa::path(
    patch,
    path = "/v1/orgs/{organization_id}/contents/{content_id}",
    params(("organization_id" = String, Path), ("content_id" = String, Path)),
    request_body = ContentInput,
    security(("bearer" = [])),
    responses(
        (status = 200, body = Content, description = "The edited content"),
        (status = 400, body = ErrorBody, description = "The request body is not JSON"),
        (status = 401, body = ErrorBody, description = "No valid access token"),
        (status = 404, body = ErrorBody, description = "No such content in the organization, or the caller is not in it"),
        (status = 413, body = ErrorBody, description = "The request body is over 2 MB"),
        (status = 415, body = ErrorBody, description = "The request body is not sent as application/json"),
        (status = 422, body = ErrorBody, description = "The request body is not valid content"),
        (status = 500, body = ErrorBody, description = "A database query failed"),
        (status = 503, body = ErrorBody, description = "Auth's signing keys could not be fetched"),
    )
)]
pub async fn update_content(
    State(state): State<AppState>,
    caller: OrgCaller,
    Path((_, content_id)): Path<(String, String)>,
    JsonBody(input): JsonBody<ContentInput>,
) -> Result<Json<Content>, ApiError> {
    sqlx::query_as(
        "UPDATE pipewhere.content SET body = $3 WHERE id = $1 AND organization_id = $2
         RETURNING id, organization_id, body",
    )
    .bind(&content_id)
    .bind(&caller.organization_id)
    .bind(input.into_body()?)
    .fetch_optional(&state.db)
    .await?
    .map(Json)
    .ok_or(ApiError::NotFound)
}
