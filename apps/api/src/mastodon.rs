//! Connecting a Mastodon account, per 00004 D3 and D4. Publishing to it is in
//! [`publish`].

use std::time::Duration;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::AppState;
use crate::access::OrgCaller;
use crate::channels::{Channel, read_channel};
use crate::error::{ApiError, ErrorBody, JsonBody};
use crate::ids::new_id;

mod publish;

pub use publish::{MastodonAdapter, PUBLISH_LIMIT};

// `write:media` is not used yet. Media will need it, and a scope added later
// takes every connected account through approval again.
const SCOPES: &str = "read:accounts write:statuses write:media";

/// Calls a Mastodon instance's API.
pub struct Mastodon {
    http: reqwest::Client,
    scheme: &'static str,
}

impl Mastodon {
    pub fn https() -> Self {
        // Connecting to an instance takes well under a second, and this allows
        // for a lost packet or two.
        Self::new("https", Duration::from_secs(5))
    }

    /// Plain HTTP, for a server on this machine, such as a test's fake
    /// instance. It accepts a connection at once or not at all.
    pub fn http() -> Self {
        Self::new("http", Duration::from_secs(1))
    }

    fn new(scheme: &'static str, connect_limit: Duration) -> Self {
        Self {
            // An operator waits on every call made while connecting.
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                // A post whose connection was never made sent nothing, so it
                // is refused, per 00004 D8. reqwest reports a connection error
                // only when this limit ends the wait, not the request's own.
                .connect_timeout(connect_limit)
                // Following a redirect sends a post again, or drops the token
                // when it leads to another host, whose 401 would revoke a
                // working channel.
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("the client builds"),
            scheme,
        }
    }

    fn url(&self, host: &str, path: &str) -> String {
        format!("{}://{host}{path}", self.scheme)
    }

    async fn register(&self, host: &str, redirect_uri: &str) -> Result<App, MastodonError> {
        let url = self.url(host, "/api/v1/apps");
        let response = self
            .http
            .post(&url)
            .form(&[
                ("client_name", "Pipewhere"),
                ("redirect_uris", redirect_uri),
                ("scopes", SCOPES),
            ])
            .send()
            .await;
        read(&url, response, &[]).await
    }

    async fn exchange(
        &self,
        host: &str,
        app: &App,
        redirect_uri: &str,
        code: &str,
    ) -> Result<String, MastodonError> {
        let url = self.url(host, "/oauth/token");
        let response = self
            .http
            .post(&url)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("client_id", &app.client_id),
                ("client_secret", &app.client_secret),
                ("redirect_uri", redirect_uri),
                ("scope", SCOPES),
            ])
            .send()
            .await;
        // Doorkeeper answers 400 for a code it does not accept, and 401 for a
        // client it does not know.
        let token: Token = read(
            &url,
            response,
            &[StatusCode::BAD_REQUEST, StatusCode::UNAUTHORIZED],
        )
        .await?;
        Ok(token.access_token)
    }

    async fn account(&self, host: &str, token: &str) -> Result<Account, MastodonError> {
        let url = self.url(host, "/api/v1/accounts/verify_credentials");
        let response = self.http.get(&url).bearer_auth(token).send().await;
        // Api::BaseController answers 403 for a disabled, unconfirmed, or
        // unapproved login, and 422 for a token with no user.
        read(
            &url,
            response,
            &[
                StatusCode::UNAUTHORIZED,
                StatusCode::FORBIDDEN,
                StatusCode::UNPROCESSABLE_ENTITY,
            ],
        )
        .await
    }
}

enum MastodonError {
    /// The instance answered with one of the statuses that mean it refused.
    Refused,
    Unavailable(String),
}

impl From<MastodonError> for ApiError {
    fn from(error: MastodonError) -> Self {
        match error {
            MastodonError::Refused => Self::AuthorizationRefused,
            MastodonError::Unavailable(reason) => Self::InstanceUnavailable(reason),
        }
    }
}

async fn read<T: serde::de::DeserializeOwned>(
    url: &str,
    response: Result<reqwest::Response, reqwest::Error>,
    refusals: &[StatusCode],
) -> Result<T, MastodonError> {
    let response =
        response.map_err(|error| MastodonError::Unavailable(format!("{url}: {error}")))?;
    let status = response.status();
    if refusals.contains(&status) {
        return Err(MastodonError::Refused);
    }
    if !status.is_success() {
        return Err(MastodonError::Unavailable(format!(
            "{url} answered {status}"
        )));
    }
    response.json().await.map_err(|error| {
        MastodonError::Unavailable(format!(
            "{url} answered {status} with a body Pipewhere cannot read: {error}"
        ))
    })
}

#[derive(Deserialize)]
struct App {
    client_id: String,
    client_secret: String,
}

#[derive(Deserialize)]
struct Token {
    access_token: String,
}

#[derive(Deserialize)]
struct Account {
    id: String,
    username: String,
}

/// The instance's host, with its port when it is not the default one. An
/// address is a host, optionally after `https://`, and nothing more.
fn instance_host(address: &str) -> Option<String> {
    let address = address.trim();
    let rest = match address.get(..8) {
        Some(scheme) if scheme.eq_ignore_ascii_case("https://") => &address[8..],
        _ => address,
    };
    if rest.is_empty() || rest.contains("://") {
        return None;
    }
    let url = Url::parse(&format!("https://{rest}")).ok()?;
    let host = url.host_str()?;
    let plain = url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none();
    if !plain {
        return None;
    }
    Some(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    })
}

fn redirect_uri(web_url: &Url) -> String {
    web_url
        .join("connect/mastodon")
        .expect("a path joins any http URL")
        .to_string()
}

// PostgreSQL text cannot hold U+0000.
fn readable_text(text: &str) -> bool {
    !text.trim().is_empty() && !text.contains('\0')
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MastodonAuthorizationRequest {
    /// The instance's address, such as `mastodon.social` or
    /// `https://mastodon.social`.
    instance: String,
}

#[derive(Serialize, ToSchema)]
pub struct MastodonAuthorization {
    /// Send it back, as `authorization_id`, to finish connecting. It works
    /// once, within an hour.
    id: String,
    /// Where the operator approves Pipewhere at the instance. The instance
    /// then sends the browser to Web's `/connect/mastodon`, with `code` and,
    /// as `state`, this authorization's id.
    authorize_url: String,
}

#[utoipa::path(
    post,
    path = "/v1/orgs/{organization_id}/channels/mastodon/authorizations",
    params(("organization_id" = String, Path)),
    request_body = MastodonAuthorizationRequest,
    security(("bearer" = [])),
    responses(
        (status = 201, body = MastodonAuthorization, description = "Where to send the operator to approve Pipewhere"),
        (status = 400, body = ErrorBody, description = "The request body is not JSON"),
        (status = 401, body = ErrorBody, description = "No valid access token"),
        (status = 404, body = ErrorBody, description = "No such organization, or the caller is not in it"),
        (status = 413, body = ErrorBody, description = "The request body is over 2 MB"),
        (status = 415, body = ErrorBody, description = "The request body is not sent as application/json"),
        (status = 422, body = ErrorBody, description = "`invalid_body`: the request is not valid, or `instance` is not an instance's address"),
        (status = 500, body = ErrorBody, description = "A database query failed"),
        (status = 502, body = ErrorBody, description = "`instance_unavailable`: the instance could not be reached, or did not register Pipewhere"),
        (status = 503, body = ErrorBody, description = "Auth's signing keys could not be fetched"),
    )
)]
pub async fn authorize(
    State(state): State<AppState>,
    caller: OrgCaller,
    JsonBody(input): JsonBody<MastodonAuthorizationRequest>,
) -> Result<(StatusCode, Json<MastodonAuthorization>), ApiError> {
    let host = instance_host(&input.instance)
        .ok_or(ApiError::InvalidBody(StatusCode::UNPROCESSABLE_ENTITY))?;
    let redirect_uri = redirect_uri(&state.web_url);
    let client_id = registered_client(&state, &host, &redirect_uri).await?;

    sqlx::query(
        "DELETE FROM pipewhere.mastodon_authorization WHERE created_at < now() - interval '1 hour'",
    )
    .execute(&state.db)
    .await?;
    let id = new_id("mastodon_authorization");
    sqlx::query(
        "INSERT INTO pipewhere.mastodon_authorization (id, organization_id, host) VALUES ($1, $2, $3)",
    )
    .bind(&id)
    .bind(&caller.organization_id)
    .bind(&host)
    .execute(&state.db)
    .await?;

    let mut authorize_url = Url::parse(&state.mastodon.url(&host, "/oauth/authorize"))
        .expect("a host from instance_host makes a URL");
    authorize_url
        .query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &client_id)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("scope", SCOPES)
        .append_pair("state", &id);
    Ok((
        StatusCode::CREATED,
        Json(MastodonAuthorization {
            id,
            authorize_url: authorize_url.into(),
        }),
    ))
}

/// Registers Pipewhere with the instance unless it already holds a client
/// for this redirect address whose secret opens.
///
/// Two first connections to one instance can both register. The first to
/// store its client wins, and both send the operator to it. The other client
/// stays registered at the instance, unused.
async fn registered_client(
    state: &AppState,
    host: &str,
    redirect_uri: &str,
) -> Result<String, ApiError> {
    let stored: Option<(String, Vec<u8>)> = sqlx::query_as(
        "SELECT client_id, client_secret FROM pipewhere.mastodon_instance
         WHERE host = $1 AND redirect_uri = $2",
    )
    .bind(host)
    .bind(redirect_uri)
    .fetch_optional(&state.db)
    .await?;
    let unreadable = match stored {
        Some((client_id, sealed)) if state.credentials.open(host, &sealed).is_ok() => {
            return Ok(client_id);
        }
        Some((_, sealed)) => {
            tracing::warn!(
                "The client secret for {host} does not open under CREDENTIALS_KEY, so API registers with {host} again. If the key was changed by mistake, restore it before connecting more channels."
            );
            Some(sealed)
        }
        None => None,
    };
    let app = state.mastodon.register(host, redirect_uri).await?;
    if !readable_text(&app.client_id) {
        return Err(ApiError::InstanceUnavailable(format!(
            "{host} registered Pipewhere without a client id"
        )));
    }
    // A client is replaced when Web's address changed, because the instance
    // would refuse to send the browser there. It is also replaced when its
    // secret no longer opens, because CREDENTIALS_KEY was changed. Matching
    // that secret lets only the first of two such connections replace it.
    sqlx::query(
        "INSERT INTO pipewhere.mastodon_instance (host, redirect_uri, client_id, client_secret)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (host) DO UPDATE
         SET redirect_uri = EXCLUDED.redirect_uri,
             client_id = EXCLUDED.client_id,
             client_secret = EXCLUDED.client_secret
         WHERE mastodon_instance.redirect_uri <> EXCLUDED.redirect_uri
            OR mastodon_instance.client_secret = $5",
    )
    .bind(host)
    .bind(redirect_uri)
    .bind(&app.client_id)
    .bind(state.credentials.seal(host, &app.client_secret))
    .bind(unreadable)
    .execute(&state.db)
    .await?;
    Ok(
        sqlx::query_scalar("SELECT client_id FROM pipewhere.mastodon_instance WHERE host = $1")
            .bind(host)
            .fetch_one(&state.db)
            .await?,
    )
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MastodonConnection {
    /// The id of the authorization that sent the operator to the instance.
    /// The instance hands it back as `state`.
    authorization_id: String,
    /// The `code` the instance handed back.
    code: String,
}

#[utoipa::path(
    post,
    path = "/v1/orgs/{organization_id}/channels/mastodon",
    params(("organization_id" = String, Path)),
    request_body = MastodonConnection,
    security(("bearer" = [])),
    responses(
        (status = 200, body = Channel, description = "The organization already linked the account. The channel holds the new access, and keeps its id and history"),
        (status = 201, body = Channel, description = "The new channel"),
        (status = 400, body = ErrorBody, description = "The request body is not JSON"),
        (status = 401, body = ErrorBody, description = "No valid access token"),
        (status = 404, body = ErrorBody, description = "No such organization, or the caller is not in it"),
        (status = 413, body = ErrorBody, description = "The request body is over 2 MB"),
        (status = 415, body = ErrorBody, description = "The request body is not sent as application/json"),
        (status = 422, body = ErrorBody, description = "`invalid_body`: the request is not valid. `authorization_not_found`: no authorization in the organization has this id, or it was used or is over an hour old. `authorization_refused`: the instance refused the code or the account"),
        (status = 500, body = ErrorBody, description = "A database query failed, or a stored secret could not be read"),
        (status = 502, body = ErrorBody, description = "`instance_unavailable`: the instance could not be reached, or did not answer as Mastodon does"),
        (status = 503, body = ErrorBody, description = "Auth's signing keys could not be fetched"),
    )
)]
pub async fn connect(
    State(state): State<AppState>,
    caller: OrgCaller,
    JsonBody(input): JsonBody<MastodonConnection>,
) -> Result<(StatusCode, Json<Channel>), ApiError> {
    if input.code.trim().is_empty() {
        return Err(ApiError::InvalidBody(StatusCode::UNPROCESSABLE_ENTITY));
    }
    if input.authorization_id.contains('\0') {
        return Err(ApiError::AuthorizationNotFound);
    }
    // Using it up first means a code handed back twice connects once.
    let host: String = sqlx::query_scalar(
        "DELETE FROM pipewhere.mastodon_authorization
         WHERE id = $1 AND organization_id = $2 AND created_at >= now() - interval '1 hour'
         RETURNING host",
    )
    .bind(&input.authorization_id)
    .bind(&caller.organization_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::AuthorizationNotFound)?;

    let (redirect_uri, client_id, sealed): (String, String, Vec<u8>) = sqlx::query_as(
        "SELECT redirect_uri, client_id, client_secret FROM pipewhere.mastodon_instance WHERE host = $1",
    )
    .bind(&host)
    .fetch_one(&state.db)
    .await?;
    let client_secret = state
        .credentials
        .open(&host, &sealed)
        .map_err(|_| ApiError::Unreadable(format!("The client secret for {host}")))?;
    let app = App {
        client_id,
        client_secret,
    };
    let token = state
        .mastodon
        .exchange(&host, &app, &redirect_uri, &input.code)
        .await?;
    let account = state.mastodon.account(&host, &token).await?;
    if !readable_text(&account.id) || !readable_text(&account.username) {
        return Err(ApiError::InstanceUnavailable(format!(
            "{host} answered an account without an id or a username"
        )));
    }
    let handle = format!("@{}@{host}", account.username);

    let mut tx = state.db.begin().await?;
    let linked: Option<String> = sqlx::query_scalar(
        "SELECT c.id FROM pipewhere.channel c
         JOIN pipewhere.mastodon_channel m ON m.channel_id = c.id
         WHERE c.organization_id = $1 AND m.host = $2 AND m.account_id = $3
         FOR UPDATE OF c",
    )
    .bind(&caller.organization_id)
    .bind(&host)
    .bind(&account.id)
    .fetch_optional(&mut *tx)
    .await?;
    let (status, channel_id) = match linked {
        Some(channel_id) => {
            sqlx::query(
                "UPDATE pipewhere.channel SET handle = $2, status = 'active' WHERE id = $1",
            )
            .bind(&channel_id)
            .bind(&handle)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "UPDATE pipewhere.mastodon_channel SET access_token = $2 WHERE channel_id = $1",
            )
            .bind(&channel_id)
            .bind(state.credentials.seal(&channel_id, &token))
            .execute(&mut *tx)
            .await?;
            (StatusCode::OK, channel_id)
        }
        None => {
            let channel_id = new_id("channel");
            sqlx::query(
                "INSERT INTO pipewhere.channel (id, organization_id, provider, handle, status)
                 VALUES ($1, $2, 'mastodon', $3, 'active')",
            )
            .bind(&channel_id)
            .bind(&caller.organization_id)
            .bind(&handle)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "INSERT INTO pipewhere.mastodon_channel (channel_id, host, account_id, access_token)
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(&channel_id)
            .bind(&host)
            .bind(&account.id)
            .bind(state.credentials.seal(&channel_id, &token))
            .execute(&mut *tx)
            .await?;
            (StatusCode::CREATED, channel_id)
        }
    };
    let channel = read_channel(&mut *tx, &caller.organization_id, &channel_id)
        .await?
        .expect("the channel was written in this transaction");
    tx.commit().await?;
    Ok((status, Json(channel)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_is_a_host_with_an_optional_https_scheme_and_port() {
        for (address, host) in [
            ("kopiti.am", "kopiti.am"),
            ("  Mastodon.Social/ ", "mastodon.social"),
            ("https://mastodon.social", "mastodon.social"),
            ("HTTPS://mastodon.social/", "mastodon.social"),
            ("mastodon.example:8443", "mastodon.example:8443"),
            ("mastodon.example:443", "mastodon.example"),
            ("127.0.0.1:4000", "127.0.0.1:4000"),
            ("bücher.example", "xn--bcher-kva.example"),
        ] {
            assert_eq!(instance_host(address).as_deref(), Some(host), "{address:?}");
        }
    }

    #[test]
    fn anything_more_than_a_host_is_not_an_address() {
        for address in [
            "",
            "   ",
            "https://",
            "http://mastodon.social",
            "ftp://mastodon.social",
            "mastodon.social/@name",
            "@name@mastodon.social",
            "name@mastodon.social",
            "mastodon.social?x=1",
            "mastodon.social#top",
            "https://https://mastodon.social",
        ] {
            assert_eq!(instance_host(address), None, "{address:?}");
        }
    }

    #[test]
    fn the_redirect_address_is_on_web() {
        let web = Url::parse("http://localhost:3000").unwrap();
        assert_eq!(redirect_uri(&web), "http://localhost:3000/connect/mastodon");
        let web = Url::parse("https://app.example.com/").unwrap();
        assert_eq!(
            redirect_uri(&web),
            "https://app.example.com/connect/mastodon"
        );
    }
}
