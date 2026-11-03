use std::collections::HashMap;

use axum::Json;
use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use utoipa::ToSchema;

use crate::AppState;
use crate::access::OrgCaller;
use crate::contents::checked_body;
use crate::error::{ApiError, ErrorBody, JsonBody};
use crate::ids::new_id;

const PAGE_SIZE: usize = 100;

/// A publication's place on the path to a remote post, per 00002 D3.
#[derive(Serialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum PublicationState {
    Scheduled,
    Preparing,
    Publishing,
    Published,
    Failed,
    OutcomeUnknown,
    Cancelled,
}

/// One snapshot of text for one channel. Editing the content it was made from
/// never changes it.
#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct Publication {
    id: String,
    organization_id: String,
    channel_id: String,
    content_id: String,
    body: String,
    state: PublicationState,
    planned_at: DateTime<Utc>,
}

#[derive(Serialize, ToSchema)]
pub struct Publications {
    publications: Vec<Publication>,
    /// Pass as `cursor` for the next page. Null on the last page.
    #[schema(required = true)]
    next_cursor: Option<String>,
}

// A misspelt `planned_at` would otherwise be dropped, and the publication
// planned for now.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PublicationInput {
    channel_id: String,
    /// Text for new content, which the publication copies. Send this or
    /// `content_id`, not both. It must hold a character other than whitespace,
    /// and no NUL character.
    body: Option<String>,
    /// Existing content, whose body the publication copies.
    content_id: Option<String>,
    /// When to publish, in a year from 0000 to 9999 in UTC. Defaults to the
    /// time of the request.
    #[serde(default, deserialize_with = "rfc3339")]
    planned_at: Option<DateTime<Utc>>,
}

// chrono's own deserializer also accepts forms RFC 3339 does not, such as
// `+0800` or single-digit fields.
fn rfc3339<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error> {
    Option::<String>::deserialize(deserializer)?
        .map(|text| {
            DateTime::parse_from_rfc3339(&text)
                .map(|time| time.to_utc())
                .map_err(serde::de::Error::custom)
        })
        .transpose()
}

impl PublicationInput {
    fn request_hash(&self) -> Vec<u8> {
        let request = (
            "create_publication",
            &self.channel_id,
            &self.body,
            &self.content_id,
            self.planned_at,
        );
        Sha256::digest(serde_json::to_vec(&request).expect("the request serializes")).to_vec()
    }
}

// PostgreSQL text cannot hold U+0000, so no id holds one, and a lookup with
// one fails as a server error.
fn holds_nul(id: &str) -> bool {
    id.contains('\0')
}

enum Source {
    Body(String),
    Content(String),
}

/// The `Idempotency-Key` header, per 00002 D5 and D13.
pub struct IdempotencyKey(String);

impl<S: Send + Sync> FromRequestParts<S> for IdempotencyKey {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        parts
            .headers
            .get("idempotency-key")
            .and_then(|value| value.to_str().ok())
            .filter(|key| (22..=255).contains(&key.len()))
            .map(|key| Self(key.to_owned()))
            .ok_or(ApiError::InvalidIdempotencyKey)
    }
}

#[utoipa::path(
    post,
    path = "/v1/orgs/{organization_id}/publications",
    params(
        ("organization_id" = String, Path),
        ("Idempotency-Key" = String, Header, description = "At least 128 random bits, such as 16 random bytes in base64url: 22 to 255 visible ASCII characters. Sending the same request with the same key returns the first result."),
    ),
    request_body = PublicationInput,
    security(("bearer" = [])),
    responses(
        (status = 201, body = Publication, description = "The publication was created, or the key's first request created it"),
        (status = 400, body = ErrorBody, description = "The request body is not JSON, or the Idempotency-Key is missing or not 22 to 255 visible ASCII characters"),
        (status = 401, body = ErrorBody, description = "No valid access token"),
        (status = 404, body = ErrorBody, description = "No such organization, or the caller is not in it"),
        (status = 413, body = ErrorBody, description = "The request body is over 2 MB"),
        (status = 415, body = ErrorBody, description = "The request body is not sent as application/json"),
        (status = 422, body = ErrorBody, description = "The request body is not a valid publication, names a channel or content not in the organization, or the key was used for a different request"),
        (status = 500, body = ErrorBody, description = "A database query failed"),
        (status = 503, body = ErrorBody, description = "Auth's signing keys could not be fetched"),
    )
)]
pub async fn create_publication(
    State(state): State<AppState>,
    caller: OrgCaller,
    IdempotencyKey(key): IdempotencyKey,
    JsonBody(input): JsonBody<PublicationInput>,
) -> Result<(StatusCode, Json<Publication>), ApiError> {
    let request_hash = input.request_hash();
    let source = match (input.body, input.content_id) {
        (Some(body), None) => Source::Body(checked_body(body)?),
        (None, Some(content_id)) => Source::Content(content_id),
        _ => return Err(ApiError::InvalidBody(StatusCode::UNPROCESSABLE_ENTITY)),
    };
    // RFC 3339 has four-digit years, and the schema publishes `date-time`.
    // The year checked is that of the whole microseconds sqlx stores, into
    // which a leap second carries.
    if input.planned_at.is_some_and(|time| {
        DateTime::from_timestamp_micros(time.timestamp_micros())
            .is_none_or(|stored| !(0..=9999).contains(&stored.year()))
    }) {
        return Err(ApiError::InvalidBody(StatusCode::UNPROCESSABLE_ENTITY));
    }
    if holds_nul(&input.channel_id) {
        return Err(ApiError::ChannelNotFound);
    }
    if matches!(&source, Source::Content(content_id) if holds_nul(content_id)) {
        return Err(ApiError::ContentNotFound);
    }
    let organization_id = &caller.organization_id;
    let id = new_id("publication");

    let mut tx = state.db.begin().await?;
    // A concurrent insert of the same key waits here until the first
    // transaction ends, so only one request goes on to create.
    let claimed = sqlx::query(
        "INSERT INTO pipewhere.idempotency_key (organization_id, key, request_hash, publication_id)
         VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
    )
    .bind(organization_id)
    .bind(&key)
    .bind(&request_hash)
    .bind(&id)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        == 1;
    if !claimed {
        tx.rollback().await?;
        let (stored_hash, publication_id): (Vec<u8>, String) = sqlx::query_as(
            "SELECT request_hash, publication_id FROM pipewhere.idempotency_key
             WHERE organization_id = $1 AND key = $2",
        )
        .bind(organization_id)
        .bind(&key)
        .fetch_one(&state.db)
        .await?;
        if stored_hash != request_hash {
            return Err(ApiError::IdempotencyKeyReused);
        }
        let publication = sqlx::query_as(
            "SELECT id, organization_id, channel_id, content_id, body, state, planned_at
             FROM pipewhere.publication WHERE id = $1 AND organization_id = $2",
        )
        .bind(&publication_id)
        .bind(organization_id)
        .fetch_one(&state.db)
        .await?;
        return Ok((StatusCode::CREATED, Json(publication)));
    }

    sqlx::query("SELECT 1 FROM pipewhere.channel WHERE id = $1 AND organization_id = $2")
        .bind(&input.channel_id)
        .bind(organization_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(ApiError::ChannelNotFound)?;
    let (content_id, body): (String, String) = match source {
        Source::Body(body) => {
            sqlx::query_as(
                "INSERT INTO pipewhere.content (id, organization_id, body) VALUES ($1, $2, $3)
                 RETURNING id, body",
            )
            .bind(new_id("content"))
            .bind(organization_id)
            .bind(body)
            .fetch_one(&mut *tx)
            .await?
        }
        Source::Content(content_id) => sqlx::query_as(
            "SELECT id, body FROM pipewhere.content WHERE id = $1 AND organization_id = $2",
        )
        .bind(&content_id)
        .bind(organization_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(ApiError::ContentNotFound)?,
    };
    let publication = sqlx::query_as(
        "INSERT INTO pipewhere.publication (id, organization_id, channel_id, content_id, body, state, planned_at)
         VALUES ($1, $2, $3, $4, $5, 'scheduled', COALESCE($6, now()))
         RETURNING id, organization_id, channel_id, content_id, body, state, planned_at",
    )
    .bind(&id)
    .bind(organization_id)
    .bind(&input.channel_id)
    .bind(&content_id)
    .bind(&body)
    .bind(input.planned_at)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(publication)))
}

#[utoipa::path(
    get,
    path = "/v1/orgs/{organization_id}/publications",
    params(
        ("organization_id" = String, Path),
        ("cursor" = Option<String>, Query, description = "The previous page's `next_cursor`"),
    ),
    security(("bearer" = [])),
    responses(
        (status = 200, body = Publications, description = "The organization's publications, newest first, 100 to a page"),
        (status = 401, body = ErrorBody, description = "No valid access token"),
        (status = 404, body = ErrorBody, description = "No such organization, or the caller is not in it"),
        (status = 500, body = ErrorBody, description = "A database query failed"),
        (status = 503, body = ErrorBody, description = "Auth's signing keys could not be fetched"),
    )
)]
pub async fn list_publications(
    State(state): State<AppState>,
    caller: OrgCaller,
    // A map cannot fail to deserialize, so no query string answers axum's
    // plain-text rejection.
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Publications>, ApiError> {
    let cursor = query.get("cursor");
    if cursor.is_some_and(|cursor| holds_nul(cursor)) {
        return Ok(Json(Publications {
            publications: Vec::new(),
            next_cursor: None,
        }));
    }
    let mut publications: Vec<Publication> = sqlx::query_as(
        "SELECT id, organization_id, channel_id, content_id, body, state, planned_at
         FROM pipewhere.publication WHERE organization_id = $1 AND ($2::text IS NULL OR id < $2)
         ORDER BY id DESC LIMIT $3",
    )
    .bind(&caller.organization_id)
    .bind(cursor)
    .bind(PAGE_SIZE as i64 + 1)
    .fetch_all(&state.db)
    .await?;
    let next_cursor = (publications.len() > PAGE_SIZE).then(|| {
        publications.truncate(PAGE_SIZE);
        publications[PAGE_SIZE - 1].id.clone()
    });
    Ok(Json(Publications {
        publications,
        next_cursor,
    }))
}

#[utoipa::path(
    get,
    path = "/v1/orgs/{organization_id}/publications/{publication_id}",
    params(("organization_id" = String, Path), ("publication_id" = String, Path)),
    security(("bearer" = [])),
    responses(
        (status = 200, body = Publication, description = "The publication"),
        (status = 401, body = ErrorBody, description = "No valid access token"),
        (status = 404, body = ErrorBody, description = "No such publication in the organization, or the caller is not in it"),
        (status = 500, body = ErrorBody, description = "A database query failed"),
        (status = 503, body = ErrorBody, description = "Auth's signing keys could not be fetched"),
    )
)]
pub async fn get_publication(
    State(state): State<AppState>,
    caller: OrgCaller,
    Path((_, publication_id)): Path<(String, String)>,
) -> Result<Json<Publication>, ApiError> {
    if holds_nul(&publication_id) {
        return Err(ApiError::NotFound);
    }
    sqlx::query_as(
        "SELECT id, organization_id, channel_id, content_id, body, state, planned_at
         FROM pipewhere.publication WHERE id = $1 AND organization_id = $2",
    )
    .bind(&publication_id)
    .bind(&caller.organization_id)
    .fetch_optional(&state.db)
    .await?
    .map(Json)
    .ok_or(ApiError::NotFound)
}
