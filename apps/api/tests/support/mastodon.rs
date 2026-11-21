//! A Mastodon instance on a local port, answering as Mastodon v4.5.20 does:
//!
//! - `POST /api/v1/apps`: `Api::V1::AppsController#create`, which takes form
//!   fields and answers the client's id and secret.
//! - `POST /oauth/token`: Doorkeeper's token endpoint. An authorization code
//!   works once, for the client and redirect address it was issued to, and
//!   any other answers 400 `invalid_grant`.
//! - `GET /api/v1/accounts/verify_credentials`: the token's account, or 401.
//! - `POST /api/v1/statuses`: `Api::V1::StatusesController#create` and
//!   `PostStatusService`. A repeated `Idempotency-Key` from one account
//!   answers the status it first posted, and posts nothing.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Form, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use pipewhere_api::credentials::Credentials;
use pipewhere_api::ids::new_id;
use reqwest::Url;
use serde_json::json;
use sqlx::PgPool;

#[derive(Clone)]
pub struct Account {
    pub id: &'static str,
    pub username: &'static str,
}

pub const STAN: Account = Account {
    id: "109384",
    username: "stan",
};

pub struct FakeMastodon {
    /// The address an operator enters for this instance.
    pub host: String,
    state: Arc<Mutex<Fake>>,
}

#[derive(Default)]
struct Fake {
    host: String,
    /// The form of every `POST /api/v1/apps`, in order.
    registrations: Vec<HashMap<String, String>>,
    codes: HashMap<String, Issued>,
    tokens: HashMap<String, Account>,
    issued: usize,
    refuse_registration: bool,
    statuses: Vec<Posted>,
    /// Every `POST /api/v1/statuses`, including those that posted nothing.
    status_requests: usize,
    /// The status each account's `Idempotency-Key` posted.
    keys: HashMap<(&'static str, String), usize>,
    answer: Answer,
}

/// A status the instance posted.
#[derive(Clone, Debug)]
pub struct Posted {
    pub id: String,
    pub account_id: &'static str,
    pub text: String,
    pub idempotency_key: Option<String>,
}

#[derive(Clone, Default)]
enum Answer {
    #[default]
    Post,
    /// Posts the status, then holds the response open, as an instance whose
    /// answer is lost does.
    PostThenHold,
    /// Answers this, and posts nothing.
    Respond(StatusCode, String),
    /// Answers 307 to this address, and posts nothing.
    Redirect(String),
}

struct Issued {
    client_id: String,
    redirect_uri: String,
    account: Account,
}

impl FakeMastodon {
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let host = listener.local_addr().unwrap().to_string();
        let state = Arc::new(Mutex::new(Fake {
            host: host.clone(),
            ..Fake::default()
        }));
        let app = Router::new()
            .route("/api/v1/apps", post(register))
            .route("/oauth/token", post(token))
            .route("/api/v1/accounts/verify_credentials", get(account))
            .route("/api/v1/statuses", post(post_status))
            .with_state(state.clone());
        tokio::spawn(async move { axum::serve(listener, app).await });
        Self { host, state }
    }

    pub fn registrations(&self) -> Vec<HashMap<String, String>> {
        self.state.lock().unwrap().registrations.clone()
    }

    /// Answers every registration with a 404, as a server that is not Mastodon
    /// would.
    pub fn refuse_registration(&self) {
        self.state.lock().unwrap().refuse_registration = true;
    }

    /// What the instance's approval page does when `account` approves: checks
    /// the request against a registered client, then issues a code. Answers
    /// the code and the `state` the redirect hands back.
    pub fn approve(&self, authorize_url: &str, account: &Account) -> (String, String) {
        let url = Url::parse(authorize_url).unwrap();
        assert_eq!(url.host_str(), Some("127.0.0.1"));
        assert_eq!(url.path(), "/oauth/authorize");
        let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(query["response_type"], "code");
        let mut fake = self.state.lock().unwrap();
        let registered = fake.registrations.iter().enumerate().any(|(i, form)| {
            client_id(i) == query["client_id"] && form["redirect_uris"] == query["redirect_uri"]
        });
        assert!(
            registered,
            "no client {} for {}",
            query["client_id"], query["redirect_uri"]
        );
        fake.issued += 1;
        let code = format!("code-{}", fake.issued);
        fake.codes.insert(
            code.clone(),
            Issued {
                client_id: query["client_id"].clone(),
                redirect_uri: query["redirect_uri"].clone(),
                account: account.clone(),
            },
        );
        (code, query["state"].clone())
    }

    /// The tokens the instance issued, for checking what Pipewhere stored.
    pub fn token_for(&self, code_number: usize) -> String {
        format!("token-{code_number}")
    }

    /// A token for `account`, as if it had approved Pipewhere earlier.
    pub fn grant(&self, account: &Account) -> String {
        let mut fake = self.state.lock().unwrap();
        let token = format!("granted-{}", fake.tokens.len());
        fake.tokens.insert(token.clone(), account.clone());
        token
    }

    /// What revoking Pipewhere at the instance does to its token.
    pub fn revoke(&self, token: &str) {
        self.state.lock().unwrap().tokens.remove(token);
    }

    pub fn statuses(&self) -> Vec<Posted> {
        self.state.lock().unwrap().statuses.clone()
    }

    pub fn status_requests(&self) -> usize {
        self.state.lock().unwrap().status_requests
    }

    pub fn post_then_hold(&self) {
        self.state.lock().unwrap().answer = Answer::PostThenHold;
    }

    pub fn post(&self) {
        self.state.lock().unwrap().answer = Answer::Post;
    }

    /// Answers every `POST /api/v1/statuses` with `status` and `body`, and
    /// posts nothing.
    pub fn respond(&self, status: StatusCode, body: &str) {
        self.state.lock().unwrap().answer = Answer::Respond(status, body.to_owned());
    }

    /// Answers every `POST /api/v1/statuses` with a 307 to `location`, and
    /// posts nothing.
    pub fn redirect(&self, location: &str) {
        self.state.lock().unwrap().answer = Answer::Redirect(location.to_owned());
    }
}

/// Links `account` on `fake` to the organization as an active channel, as
/// connecting it would, and answers the channel's id and its token.
pub async fn link(
    db: &PgPool,
    organization_id: &str,
    fake: &FakeMastodon,
    account: &Account,
) -> (String, String) {
    let credentials = Credentials::from_hex(super::CREDENTIALS_KEY).unwrap();
    let token = fake.grant(account);
    let channel_id = new_id("channel");
    sqlx::query(
        "INSERT INTO pipewhere.mastodon_instance (host, redirect_uri, client_id, client_secret)
         VALUES ($1, 'http://localhost:3000/connect/mastodon', 'client', $2)
         ON CONFLICT (host) DO NOTHING",
    )
    .bind(&fake.host)
    .bind(credentials.seal(&fake.host, "secret"))
    .execute(db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO pipewhere.channel (id, organization_id, provider, handle, status)
         VALUES ($1, $2, 'mastodon', $3, 'active')",
    )
    .bind(&channel_id)
    .bind(organization_id)
    .bind(format!("@{}@{}", account.username, fake.host))
    .execute(db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO pipewhere.mastodon_channel (channel_id, host, account_id, access_token)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(&channel_id)
    .bind(&fake.host)
    .bind(account.id)
    .bind(credentials.seal(&channel_id, &token))
    .execute(db)
    .await
    .unwrap();
    (channel_id, token)
}

fn client_id(registration: usize) -> String {
    format!("client-{registration}")
}

async fn register(
    State(state): State<Arc<Mutex<Fake>>>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let mut fake = state.lock().unwrap();
    if fake.refuse_registration {
        return StatusCode::NOT_FOUND.into_response();
    }
    let n = fake.registrations.len();
    fake.registrations.push(form.clone());
    Json(json!({
        "id": n.to_string(),
        "name": form["client_name"],
        "website": null,
        "scopes": form["scopes"].split(' ').collect::<Vec<_>>(),
        "redirect_uri": form["redirect_uris"],
        "redirect_uris": [form["redirect_uris"]],
        "client_id": client_id(n),
        "client_secret": format!("secret-{n}"),
        "client_secret_expires_at": 0,
    }))
    .into_response()
}

async fn token(
    State(state): State<Arc<Mutex<Fake>>>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let mut fake = state.lock().unwrap();
    let secret_matches = form["client_id"]
        .strip_prefix("client-")
        .is_some_and(|n| form["client_secret"] == format!("secret-{n}"));
    let issued = fake.codes.get(&form["code"]).filter(|issued| {
        form["grant_type"] == "authorization_code"
            && issued.client_id == form["client_id"]
            && issued.redirect_uri == form["redirect_uri"]
            && secret_matches
    });
    let Some(issued) = issued else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_grant", "error_description": "The provided authorization grant is invalid, expired, revoked, does not match the redirection URI used in the authorization request, or was issued to another client." })),
        )
            .into_response();
    };
    let account = issued.account.clone();
    let number = form["code"].trim_start_matches("code-").to_owned();
    fake.codes.remove(&form["code"]);
    let access_token = format!("token-{number}");
    fake.tokens.insert(access_token.clone(), account);
    Json(json!({
        "access_token": access_token,
        "token_type": "Bearer",
        "scope": form["scope"],
        "created_at": 1_762_646_400,
    }))
    .into_response()
}

async fn account(State(state): State<Arc<Mutex<Fake>>>, headers: HeaderMap) -> Response {
    let fake = state.lock().unwrap();
    match bearer_account(&fake, &headers) {
        Some(account) => Json(json!({
            "id": account.id,
            "username": account.username,
            "acct": account.username,
            "display_name": account.username,
        }))
        .into_response(),
        None => (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "The access token is invalid" })),
        )
            .into_response(),
    }
}

fn bearer_account(fake: &Fake, headers: &HeaderMap) -> Option<Account> {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok()?.strip_prefix("Bearer "))
        .and_then(|token| fake.tokens.get(token))
        .cloned()
}

fn status_json(fake: &Fake, posted: &Posted, username: &str) -> serde_json::Value {
    json!({
        "id": posted.id,
        "uri": format!("http://{}/users/{username}/statuses/{}", fake.host, posted.id),
        "url": format!("http://{}/@{username}/{}", fake.host, posted.id),
        "content": format!("<p>{}</p>", posted.text),
        "visibility": "public",
    })
}

async fn post_status(
    State(state): State<Arc<Mutex<Fake>>>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let (answer, hold) = {
        let mut fake = state.lock().unwrap();
        fake.status_requests += 1;
        let Some(account) = bearer_account(&fake, &headers) else {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "The access token is invalid" })),
            )
                .into_response();
        };
        match &fake.answer {
            Answer::Respond(status, body) => return (*status, body.clone()).into_response(),
            Answer::Redirect(location) => {
                return (
                    StatusCode::TEMPORARY_REDIRECT,
                    [("location", location.clone())],
                )
                    .into_response();
            }
            Answer::Post | Answer::PostThenHold => {}
        }
        let key = headers
            .get("idempotency-key")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let earlier = key
            .as_ref()
            .and_then(|key| fake.keys.get(&(account.id, key.clone())).copied());
        let index = match earlier {
            Some(index) => index,
            None => {
                let posted = Posted {
                    id: format!("1100{}", fake.statuses.len()),
                    account_id: account.id,
                    text: form.get("status").cloned().unwrap_or_default(),
                    idempotency_key: key.clone(),
                };
                fake.statuses.push(posted);
                let index = fake.statuses.len() - 1;
                if let Some(key) = key {
                    fake.keys.insert((account.id, key), index);
                }
                index
            }
        };
        let answer = status_json(&fake, &fake.statuses[index], account.username);
        (answer, matches!(fake.answer, Answer::PostThenHold))
    };
    if hold {
        std::future::pending::<()>().await;
    }
    Json(answer).into_response()
}
