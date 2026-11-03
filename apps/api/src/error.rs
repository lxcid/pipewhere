use axum::Json;
use axum::extract::{FromRequest, Request};
use axum::http::StatusCode;
use axum::http::header::WWW_AUTHENTICATE;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

#[derive(Serialize, utoipa::ToSchema)]
pub struct ErrorBody {
    code: &'static str,
}

#[derive(Debug)]
pub enum ApiError {
    Unauthorized,
    NotFound,
    MethodNotAllowed,
    /// Carries the status axum's `Json` extractor chose: 400 for malformed
    /// JSON, 413 over the body limit, 415 without a JSON content type, and
    /// 422 for JSON of the wrong shape.
    InvalidBody(StatusCode),
    AuthUnavailable(String),
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
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Self::MethodNotAllowed => (StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed"),
            Self::InvalidBody(status) => (*status, "invalid_body"),
            Self::AuthUnavailable(reason) => {
                tracing::error!(%reason, "fetching Auth's signing keys failed");
                (StatusCode::SERVICE_UNAVAILABLE, "auth_unavailable")
            }
            Self::Database(error) => {
                tracing::error!(%error, "database query failed");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal")
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
