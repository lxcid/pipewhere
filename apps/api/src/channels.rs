use axum::Json;
use axum::extract::{Path, State};
use serde::Serialize;
use sqlx::PgExecutor;
use utoipa::ToSchema;

use crate::AppState;
use crate::access::OrgCaller;
use crate::error::{ApiError, ErrorBody};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum Provider {
    Mastodon,
    Threads,
}

/// Whether a channel can publish, and if not, why, per 00004 D2.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum ChannelStatus {
    /// Pipewhere holds access to the account and can publish to it.
    Active,
    /// Pipewhere holds no access to the account.
    Disconnected,
    /// The provider no longer accepts Pipewhere's access to the account.
    /// Connect the account again.
    AccessRevoked,
}

/// One organization's link to one external account.
#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct Channel {
    id: String,
    organization_id: String,
    provider: Provider,
    /// The account's name at its provider, such as `@name@mastodon.social`.
    /// Null for a channel that was never connected.
    handle: Option<String>,
    status: ChannelStatus,
}

pub(crate) async fn read_channel(
    db: impl PgExecutor<'_>,
    organization_id: &str,
    channel_id: &str,
) -> Result<Option<Channel>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, organization_id, provider, handle, status FROM pipewhere.channel
         WHERE id = $1 AND organization_id = $2",
    )
    .bind(channel_id)
    .bind(organization_id)
    .fetch_optional(db)
    .await
}

#[derive(Serialize, ToSchema)]
pub struct Channels {
    channels: Vec<Channel>,
}

#[utoipa::path(
    get,
    path = "/v1/orgs/{organization_id}/channels",
    params(("organization_id" = String, Path)),
    security(("bearer" = [])),
    responses(
        (status = 200, body = Channels, description = "The organization's channels, oldest first"),
        (status = 401, body = ErrorBody, description = "No valid access token"),
        (status = 404, body = ErrorBody, description = "No such organization, or the caller is not in it"),
        (status = 500, body = ErrorBody, description = "A database query failed"),
        (status = 503, body = ErrorBody, description = "Auth's signing keys could not be fetched"),
    )
)]
pub async fn list_channels(
    State(state): State<AppState>,
    caller: OrgCaller,
) -> Result<Json<Channels>, ApiError> {
    let channels = sqlx::query_as(
        "SELECT id, organization_id, provider, handle, status FROM pipewhere.channel
         WHERE organization_id = $1 ORDER BY id",
    )
    .bind(&caller.organization_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(Channels { channels }))
}

#[utoipa::path(
    get,
    path = "/v1/orgs/{organization_id}/channels/{channel_id}",
    params(("organization_id" = String, Path), ("channel_id" = String, Path)),
    security(("bearer" = [])),
    responses(
        (status = 200, body = Channel, description = "The channel"),
        (status = 401, body = ErrorBody, description = "No valid access token"),
        (status = 404, body = ErrorBody, description = "No such channel in the organization, or the caller is not in it"),
        (status = 500, body = ErrorBody, description = "A database query failed"),
        (status = 503, body = ErrorBody, description = "Auth's signing keys could not be fetched"),
    )
)]
pub async fn get_channel(
    State(state): State<AppState>,
    caller: OrgCaller,
    Path((_, channel_id)): Path<(String, String)>,
) -> Result<Json<Channel>, ApiError> {
    read_channel(&state.db, &caller.organization_id, &channel_id)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}
