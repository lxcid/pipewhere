use axum::Json;
use axum::http::StatusCode;
use axum::http::header::WWW_AUTHENTICATE;
use axum::response::{IntoResponse, Response};
use serde_json::json;

#[derive(Debug)]
pub enum ApiError {
    Unauthorized,
    NotFound,
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
            Self::AuthUnavailable(reason) => {
                tracing::error!(%reason, "fetching Auth's signing keys failed");
                (StatusCode::SERVICE_UNAVAILABLE, "auth_unavailable")
            }
            Self::Database(error) => {
                tracing::error!(%error, "database query failed");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal")
            }
        };
        let body = Json(json!({ "code": code }));
        if status == StatusCode::UNAUTHORIZED {
            (status, [(WWW_AUTHENTICATE, "Bearer")], body).into_response()
        } else {
            (status, body).into_response()
        }
    }
}
