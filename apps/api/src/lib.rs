use std::sync::Arc;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use sqlx::PgPool;
use utoipa::{OpenApi, ToSchema};

use crate::access::{Actor, OrgCaller};
use crate::error::{ApiError, ErrorBody};
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
        .route("/openapi.json", get(async || Json(ApiDoc::openapi())))
        .route("/v1/orgs/{organization_id}", get(organization))
        .fallback(async || ApiError::NotFound)
        .method_not_allowed_fallback(async || ApiError::MethodNotAllowed)
        .with_state(state)
}

pub async fn migrate(db: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(db).await
}

#[derive(OpenApi)]
#[openapi(
    info(title = "Pipewhere API", description = "Pipewhere's REST API.", license(name = "Elastic License 2.0", identifier = "Elastic-2.0")),
    paths(health, organization),
    modifiers(&BearerAuth)
)]
pub struct ApiDoc;

struct BearerAuth;

impl utoipa::Modify for BearerAuth {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
        openapi
            .components
            .get_or_insert_with(Default::default)
            .add_security_scheme(
                "bearer",
                SecurityScheme::Http(HttpBuilder::new().scheme(HttpAuthScheme::Bearer).build()),
            );
    }
}

#[derive(Serialize, ToSchema)]
struct Health {
    ok: bool,
}

#[utoipa::path(get, path = "/health", responses((status = 200, body = Health)))]
async fn health() -> Json<Health> {
    Json(Health { ok: true })
}

#[derive(Serialize, ToSchema)]
struct Organization {
    id: String,
    name: String,
    slug: String,
    actor: Actor,
}

#[utoipa::path(
    get,
    path = "/v1/orgs/{organization_id}",
    params(("organization_id" = String, Path)),
    security(("bearer" = [])),
    responses(
        (status = 200, body = Organization),
        (status = 401, body = ErrorBody, description = "No valid access token"),
        (status = 404, body = ErrorBody, description = "No such organization, or the caller is not in it"),
        (status = 500, body = ErrorBody, description = "A database query failed"),
        (status = 503, body = ErrorBody, description = "Auth's signing keys could not be fetched"),
    )
)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_schema_lists_both_endpoints() {
        let schema = serde_json::to_value(ApiDoc::openapi()).unwrap();
        let mut paths: Vec<_> = schema["paths"].as_object().unwrap().keys().collect();
        paths.sort();
        assert_eq!(paths, ["/health", "/v1/orgs/{organization_id}"]);
    }
}
