use std::sync::Arc;

use axum::extract::State;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::http::{HeaderName, HeaderValue, Method};
use axum::routing::get;
use axum::{Json, Router};
use reqwest::Url;
use serde::Serialize;
use sqlx::PgPool;
use tower_http::cors::CorsLayer;
use utoipa::{OpenApi, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::access::{Actor, OrgCaller};
use crate::credentials::Credentials;
use crate::error::{ApiError, ErrorBody};
use crate::mastodon::Mastodon;
use crate::tokens::TokenVerifier;

pub mod access;
pub mod channels;
pub mod contents;
pub mod credentials;
pub mod error;
pub mod ids;
pub mod mastodon;
pub mod publications;
pub mod runs;
pub mod settings;
pub mod tokens;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub tokens: Arc<TokenVerifier>,
    pub credentials: Arc<Credentials>,
    pub mastodon: Arc<Mastodon>,
    /// Where a browser reaches Web.
    pub web_url: Url,
}

pub fn router(state: AppState) -> Router {
    let (router, openapi) = api().split_for_parts();
    let cors = cors(&state.web_url);
    router
        .route("/openapi.json", get(async move || Json(openapi)))
        .fallback(async || ApiError::NotFound)
        .method_not_allowed_fallback(async || ApiError::MethodNotAllowed)
        .with_state(state)
        .layer(cors)
}

// Web's pages call API from the browser, with a bearer token and no cookie,
// per 00004 D6.
fn cors(web_url: &Url) -> CorsLayer {
    let origin = HeaderValue::from_str(&web_url.origin().ascii_serialization())
        .expect("an origin is a valid header value");
    CorsLayer::new()
        .allow_origin(origin)
        .allow_methods([Method::GET, Method::POST, Method::PATCH])
        .allow_headers([
            AUTHORIZATION,
            CONTENT_TYPE,
            HeaderName::from_static("idempotency-key"),
        ])
}

pub async fn migrate(db: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(db).await
}

// A handler registered with `routes!` is served and documented by that one
// call. `OpenApiRouter::route` serves without documenting, so routes here go
// through `routes!`.
fn api() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(health))
        .routes(routes!(organization))
        .routes(routes!(channels::list_channels))
        .routes(routes!(channels::get_channel))
        .routes(routes!(mastodon::authorize))
        .routes(routes!(mastodon::connect))
        .routes(routes!(contents::create_content))
        .routes(routes!(contents::get_content, contents::update_content))
        .routes(routes!(
            publications::create_publication,
            publications::list_publications
        ))
        .routes(routes!(
            publications::get_publication,
            publications::update_publication
        ))
        .routes(routes!(publications::cancel_publication))
        .routes(routes!(publications::retry_publication))
        .routes(routes!(publications::resolve_publication))
        .routes(routes!(publications::list_publication_attempts))
        .routes(routes!(settings::get_settings, settings::update_settings))
}

#[derive(OpenApi)]
#[openapi(
    info(title = "Pipewhere API", description = "Pipewhere's REST API.", license(name = "Elastic License 2.0", identifier = "Elastic-2.0")),
    modifiers(&BearerAuth)
)]
struct ApiDoc;

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

#[utoipa::path(get, path = "/health", responses((status = 200, body = Health, description = "API is up")))]
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
        (status = 200, body = Organization, description = "The organization and the caller"),
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
    fn the_schema_lists_every_endpoint() {
        let schema = serde_json::to_value(api().into_openapi()).unwrap();
        let mut paths: Vec<_> = schema["paths"].as_object().unwrap().keys().collect();
        paths.sort();
        assert_eq!(
            paths,
            [
                "/health",
                "/v1/orgs/{organization_id}",
                "/v1/orgs/{organization_id}/channels",
                "/v1/orgs/{organization_id}/channels/mastodon",
                "/v1/orgs/{organization_id}/channels/mastodon/authorizations",
                "/v1/orgs/{organization_id}/channels/{channel_id}",
                "/v1/orgs/{organization_id}/contents",
                "/v1/orgs/{organization_id}/contents/{content_id}",
                "/v1/orgs/{organization_id}/publications",
                "/v1/orgs/{organization_id}/publications/{publication_id}",
                "/v1/orgs/{organization_id}/publications/{publication_id}/attempts",
                "/v1/orgs/{organization_id}/publications/{publication_id}/cancel",
                "/v1/orgs/{organization_id}/publications/{publication_id}/resolve",
                "/v1/orgs/{organization_id}/publications/{publication_id}/retry",
                "/v1/orgs/{organization_id}/settings",
            ]
        );
    }

    #[test]
    fn the_schema_publishes_the_overdue_filter() {
        let schema = serde_json::to_value(api().into_openapi()).unwrap();
        let list = &schema["paths"]["/v1/orgs/{organization_id}/publications"]["get"];
        let overdue = list["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .find(|parameter| parameter["name"] == "overdue")
            .unwrap();
        assert_eq!(overdue["in"], "query");
        assert_eq!(overdue["required"], false);
        assert!(list["responses"]["422"].is_object());
    }

    #[test]
    fn the_schema_publishes_the_error_codes_and_states_as_fixed_sets() {
        let schema = serde_json::to_value(api().into_openapi()).unwrap();
        let schemas = &schema["components"]["schemas"];
        assert_eq!(
            schemas["ErrorCode"]["enum"],
            serde_json::json!([
                "unauthorized",
                "not_found",
                "method_not_allowed",
                "invalid_body",
                "invalid_query",
                "invalid_idempotency_key",
                "idempotency_key_reused",
                "channel_not_found",
                "content_not_found",
                "state_conflict",
                "authorization_not_found",
                "authorization_refused",
                "instance_unavailable",
                "auth_unavailable",
                "internal",
            ])
        );
        assert_eq!(
            schemas["ChannelStatus"]["enum"],
            serde_json::json!(["active", "disconnected"])
        );
        assert_eq!(
            schemas["PublicationState"]["enum"],
            serde_json::json!([
                "scheduled",
                "preparing",
                "publishing",
                "published",
                "failed",
                "outcome_unknown",
                "cancelled",
            ])
        );
        assert_eq!(
            schemas["ResolutionOutcome"]["enum"],
            serde_json::json!(["published", "failed"])
        );
        assert_eq!(
            schemas["AttemptResult"]["enum"],
            serde_json::json!(["published", "failed", "outcome_unknown", "cancelled"])
        );
        assert_eq!(
            schemas["Publication"]["properties"]["planned_at"]["format"],
            "date-time"
        );
        assert_eq!(
            schemas["Publication"]["properties"]["deadline"]["format"],
            "date-time"
        );
        for (schema, required) in [
            ("Publication", true),
            ("PublicationInput", false),
            ("PublicationEdit", false),
            ("OrganizationSettings", true),
            ("OrganizationSettingsEdit", true),
        ] {
            let tolerance = &schemas[schema]["properties"]["tolerance_seconds"];
            assert_eq!(
                (&tolerance["minimum"], &tolerance["maximum"]),
                (&serde_json::json!(300), &serde_json::json!(86400)),
                "{schema}"
            );
            let listed = |field: &str| {
                schemas[schema]["required"]
                    .as_array()
                    .is_some_and(|fields| fields.contains(&serde_json::json!(field)))
            };
            assert_eq!(listed("tolerance_seconds"), required, "{schema}");
            if schema == "Publication" {
                assert!(listed("deadline"));
            }
        }
    }
}
