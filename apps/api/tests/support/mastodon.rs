//! A Mastodon instance on a local port, answering as Mastodon v4.5.20 does:
//!
//! - `POST /api/v1/apps`: `Api::V1::AppsController#create`, which takes form
//!   fields and answers the client's id and secret.
//! - `POST /oauth/token`: Doorkeeper's token endpoint. An authorization code
//!   works once, for the client and redirect address it was issued to, and
//!   any other answers 400 `invalid_grant`.
//! - `GET /api/v1/accounts/verify_credentials`: the token's account, or 401.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Form, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use reqwest::Url;
use serde_json::json;

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
    /// The form of every `POST /api/v1/apps`, in order.
    registrations: Vec<HashMap<String, String>>,
    codes: HashMap<String, Issued>,
    tokens: HashMap<String, Account>,
    issued: usize,
    refuse_registration: bool,
}

struct Issued {
    client_id: String,
    redirect_uri: String,
    account: Account,
}

impl FakeMastodon {
    pub async fn start() -> Self {
        let state = Arc::new(Mutex::new(Fake::default()));
        let app = Router::new()
            .route("/api/v1/apps", post(register))
            .route("/oauth/token", post(token))
            .route("/api/v1/accounts/verify_credentials", get(account))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let host = listener.local_addr().unwrap().to_string();
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
    let account = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok()?.strip_prefix("Bearer "))
        .and_then(|token| fake.tokens.get(token));
    match account {
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
