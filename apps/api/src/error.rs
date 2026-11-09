use axum::Json;
use axum::extract::{FromRequest, Request};
use axum::http::StatusCode;
use axum::http::header::WWW_AUTHENTICATE;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

#[derive(Serialize, utoipa::ToSchema)]
pub struct ErrorBody {
    code: ErrorCode,
}

/// Every code API answers with. A client can match on the whole set.
#[derive(Clone, Copy, Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Unauthorized,
    NotFound,
    MethodNotAllowed,
    InvalidBody,
    InvalidQuery,
    InvalidIdempotencyKey,
    IdempotencyKeyReused,
    ChannelNotFound,
    ContentNotFound,
    StateConflict,
    AuthorizationNotFound,
    AuthorizationRefused,
    InstanceUnavailable,
    AuthUnavailable,
    Internal,
}

#[derive(Debug)]
pub enum ApiError {
    Unauthorized,
    NotFound,
    MethodNotAllowed,
    /// A 4xx status: the one axum's `Json` extractor chose (400 for malformed
    /// JSON, 413 over the body limit, 415 without a JSON content type, 422
    /// for JSON of the wrong shape), or 422 for a body an endpoint's own
    /// checks refuse.
    InvalidBody(StatusCode),
    /// A query parameter holds a value the endpoint cannot read.
    InvalidQuery,
    InvalidIdempotencyKey,
    IdempotencyKeyReused,
    ChannelNotFound,
    ContentNotFound,
    /// The publication's state does not allow the action, per 00002 D3.
    StateConflict,
    /// No connection under way in the organization has this id, per 00004 D4.
    AuthorizationNotFound,
    /// The instance refused the authorization code, or the account it names.
    AuthorizationRefused,
    /// The instance could not be reached, or did not answer as Mastodon does.
    InstanceUnavailable(String),
    AuthUnavailable(String),
    /// A stored secret does not open under `CREDENTIALS_KEY`, per 00004 D5.
    Unreadable(String),
    Database(sqlx::Error),
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, ErrorCode::Unauthorized),
            Self::NotFound => (StatusCode::NOT_FOUND, ErrorCode::NotFound),
            Self::MethodNotAllowed => (StatusCode::METHOD_NOT_ALLOWED, ErrorCode::MethodNotAllowed),
            Self::InvalidBody(status) => (*status, ErrorCode::InvalidBody),
            Self::InvalidQuery => (StatusCode::UNPROCESSABLE_ENTITY, ErrorCode::InvalidQuery),
            Self::InvalidIdempotencyKey => {
                (StatusCode::BAD_REQUEST, ErrorCode::InvalidIdempotencyKey)
            }
            Self::IdempotencyKeyReused => (
                StatusCode::UNPROCESSABLE_ENTITY,
                ErrorCode::IdempotencyKeyReused,
            ),
            Self::ChannelNotFound => (StatusCode::UNPROCESSABLE_ENTITY, ErrorCode::ChannelNotFound),
            Self::ContentNotFound => (StatusCode::UNPROCESSABLE_ENTITY, ErrorCode::ContentNotFound),
            Self::StateConflict => (StatusCode::CONFLICT, ErrorCode::StateConflict),
            Self::AuthorizationNotFound => (
                StatusCode::UNPROCESSABLE_ENTITY,
                ErrorCode::AuthorizationNotFound,
            ),
            Self::AuthorizationRefused => (
                StatusCode::UNPROCESSABLE_ENTITY,
                ErrorCode::AuthorizationRefused,
            ),
            Self::InstanceUnavailable(reason) => {
                tracing::warn!(%reason, "a Mastodon instance did not answer as expected");
                (StatusCode::BAD_GATEWAY, ErrorCode::InstanceUnavailable)
            }
            Self::AuthUnavailable(reason) => {
                tracing::error!(%reason, "fetching Auth's signing keys failed");
                (StatusCode::SERVICE_UNAVAILABLE, ErrorCode::AuthUnavailable)
            }
            Self::Unreadable(what) => {
                tracing::error!(
                    "{what} does not open under CREDENTIALS_KEY. Was the key changed? Restore the key it was sealed under."
                );
                (StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::Internal)
            }
            Self::Database(error) => {
                tracing::error!(%error, "database query failed");
                (StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::Internal)
            }
        };
        let body = Json(ErrorBody { code });
        if status == StatusCode::UNAUTHORIZED {
            (status, [(WWW_AUTHENTICATE, "Bearer")], body).into_response()
        } else {
            (status, body).into_response()
        }
    }
}

/// A JSON request body. axum's `Json` refuses a body it cannot read with a
/// plain-text response; this answers with API's error body instead.
pub struct JsonBody<T>(pub T);

impl<S, T> FromRequest<S> for JsonBody<T>
where
    Json<T>: FromRequest<S, Rejection = axum::extract::rejection::JsonRejection>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(request, state)
            .await
            .map_err(|rejection| ApiError::InvalidBody(rejection.status()))?;
        Ok(Self(value))
    }
}
