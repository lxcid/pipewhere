use std::collections::HashMap;

use axum::extract::{FromRef, FromRequestParts, Path};
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use serde::Serialize;
use sqlx::PgPool;

use crate::AppState;
use crate::error::ApiError;
use crate::tokens::TokenError;

/// Who is making a request, as Auth's tables say now. The token only names
/// the subject; membership, role, and a key's standing are read per request.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Actor {
    User { id: String, role: String },
    ApiKey { id: String },
}

/// A request authorized to act in the organization its path names.
///
/// Every organization-scoped handler takes this extractor, so the caller is
/// checked against the organization before the handler reads or writes.
pub struct OrgCaller {
    pub organization_id: String,
    pub actor: Actor,
}

impl<S> FromRequestParts<S> for OrgCaller
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let state = AppState::from_ref(state);
        let token = bearer_token(parts).ok_or(ApiError::Unauthorized)?;
        let claims = state
            .tokens
            .verify(token)
            .await
            .map_err(|error| match error {
                TokenError::Invalid => ApiError::Unauthorized,
                TokenError::AuthUnavailable(reason) => ApiError::AuthUnavailable(reason),
            })?;
        let Path(params) = Path::<HashMap<String, String>>::from_request_parts(parts, &state)
            .await
            .map_err(|_| ApiError::NotFound)?;
        let organization_id = params
            .get("organization_id")
            .ok_or(ApiError::NotFound)?
            .clone();
        let actor = authorize(&state.db, &claims.sub, &organization_id).await?;
        Ok(Self {
            organization_id,
            actor,
        })
    }
}

fn bearer_token(parts: &Parts) -> Option<&str> {
    let value = parts.headers.get(AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then_some(token.trim())
}

/// Checks the token's subject against the organization, in 00001 D2's order:
/// a subject that no longer authenticates is unauthorized before an
/// organization it cannot act in is not found.
async fn authorize(db: &PgPool, sub: &str, organization_id: &str) -> Result<Actor, ApiError> {
    if sub.starts_with("usr_") {
        let role: Option<String> = sqlx::query_scalar(
            r#"SELECT role FROM auth.member WHERE "organizationId" = $1 AND "userId" = $2"#,
        )
        .bind(organization_id)
        .bind(sub)
        .fetch_optional(db)
        .await?;
        let role = role.ok_or(ApiError::NotFound)?;
        Ok(Actor::User {
            id: sub.to_owned(),
            role,
        })
    } else if sub.starts_with("key_") {
        let owner: Option<String> = sqlx::query_scalar(
            r#"SELECT "referenceId" FROM auth.apikey
               WHERE id = $1 AND enabled AND ("expiresAt" IS NULL OR "expiresAt" > now())"#,
        )
        .bind(sub)
        .fetch_optional(db)
        .await?;
        let owner = owner.ok_or(ApiError::Unauthorized)?;
        if owner != organization_id {
            return Err(ApiError::NotFound);
        }
        Ok(Actor::ApiKey { id: sub.to_owned() })
    } else {
        Err(ApiError::Unauthorized)
    }
}
