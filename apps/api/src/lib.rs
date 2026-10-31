use std::sync::Arc;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::PgPool;

use crate::access::{Actor, OrgCaller};
use crate::error::ApiError;
use crate::tokens::TokenVerifier;

pub mod access;
pub mod error;
pub mod tokens;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub tokens: Arc<TokenVerifier>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/orgs/{organization_id}", get(organization))
        .fallback(async || ApiError::NotFound)
        .method_not_allowed_fallback(async || ApiError::MethodNotAllowed)
        .with_state(state)
}

pub async fn migrate(db: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(db).await
}

async fn health() -> Json<Value> {
    Json(json!({ "ok": true }))
}

#[derive(Serialize)]
struct Organization {
    id: String,
    name: String,
    slug: String,
    actor: Actor,
}

async fn organization(
    State(state): State<AppState>,
    caller: OrgCaller,
) -> Result<Json<Organization>, ApiError> {
    let (id, name, slug): (String, String, String) =
        sqlx::query_as("SELECT id, name, slug FROM auth.organization WHERE id = $1")
            .bind(&caller.organization_id)
            .fetch_optional(&state.db)
            .await?
            .ok_or(ApiError::NotFound)?;
    Ok(Json(Organization {
        id,
        name,
        slug,
        actor: caller.actor,
    }))
}
