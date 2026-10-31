//! Runs API against a real Auth: a disposable database with `setup.sql` and
//! Auth's migrations applied, and Auth's own server signing the tokens. A
//! Better Auth upgrade that changes what API reads or verifies fails here.

#![allow(dead_code)]

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pipewhere_api::tokens::TokenVerifier;
use pipewhere_api::{AppState, migrate, router};
use reqwest::{Client, Response, StatusCode, Url};
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use sqlx::{AssertSqlSafe, PgPool};
use tokio::process::{Child, Command};

pub const WEB_URL: &str = "http://localhost:3000";
const AUTH_PASSWORD: &str = "auth-test";
const API_PASSWORD: &str = "api-test";
const SECRET: &str = "0123456789abcdef0123456789abcdef0123456789abcdef";

static DATABASES: AtomicUsize = AtomicUsize::new(0);
// setup.sql creates and alters roles, which the whole server shares, so
// tests running in parallel would race on them.
static SETUP: Mutex<()> = Mutex::new(());

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

// A superuser on a disposable server. `pnpm --filter @pipewhere/auth test:db` starts one.
fn admin_url(database: &str) -> String {
    let base = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:test@127.0.0.1:5433/postgres".into());
    let mut url = Url::parse(&base).expect("TEST_DATABASE_URL is a URL");
    url.set_path(database);
    url.to_string()
}

fn role_url(database: &str, role: &str, password: &str) -> String {
    let mut url = Url::parse(&admin_url(database)).unwrap();
    url.set_username(role).unwrap();
    url.set_password(Some(password)).unwrap();
    url.to_string()
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .expect("a free local port")
        .port()
}

pub struct Instance {
    pub http: Client,
    pub api_url: String,
    pub auth_url: String,
    /// The test database as its superuser, for changes Auth has no endpoint for.
    pub admin: PgPool,
    pub api_db: PgPool,
    database: String,
    auth_env: Vec<(&'static str, String)>,
    auth: Child,
}

impl Instance {
    pub async fn start() -> Self {
        let database = format!(
            "pipewhere_api_test_{}_{}",
            std::process::id(),
            DATABASES.fetch_add(1, Ordering::Relaxed)
        );
        let server = PgPoolOptions::new()
            .max_connections(1)
            .connect(&admin_url("postgres"))
            .await
            .expect("the test PostgreSQL server is running; see the README");
        sqlx::raw_sql(AssertSqlSafe(format!("CREATE DATABASE {database}")))
            .execute(&server)
            .await
            .unwrap();
        server.close().await;

        let guard = SETUP
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let setup = std::process::Command::new("psql")
            .args(["-q", "-v", &format!("auth_password={AUTH_PASSWORD}")])
            .args(["-v", &format!("api_password={API_PASSWORD}")])
            .arg("-f")
            .arg(repo_root().join("infra/postgres/setup.sql"))
            .arg(admin_url(&database))
            .status()
            .expect("psql runs");
        drop(guard);
        assert!(setup.success(), "setup.sql failed");

        let auth_port = free_port();
        let auth_url = format!("http://127.0.0.1:{auth_port}");
        let auth_env = vec![
            (
                "DATABASE_URL",
                role_url(&database, "pipewhere_auth", AUTH_PASSWORD),
            ),
            ("BETTER_AUTH_SECRET", SECRET.to_owned()),
            ("AUTH_URL", auth_url.clone()),
            ("WEB_URL", WEB_URL.to_owned()),
            ("PORT", auth_port.to_string()),
        ];
        let migrated = node(&auth_env)
            .arg("src/migrate.ts")
            .status()
            .await
            .expect("node runs");
        assert!(migrated.success(), "Auth's migrations failed");
        let auth = node(&auth_env)
            .arg("src/server.ts")
            .stdout(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .expect("Auth starts");

        let api_db = PgPoolOptions::new()
            .connect(&role_url(&database, "pipewhere_api", API_PASSWORD))
            .await
            .unwrap();
        migrate(&api_db).await.expect("API's migrations apply");
        let tokens = TokenVerifier::new(&auth_url, format!("{auth_url}/api/auth/jwks"));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api_url = format!("http://{}", listener.local_addr().unwrap());
        let app = router(AppState {
            db: api_db.clone(),
            tokens: Arc::new(tokens),
        });
        tokio::spawn(async move { axum::serve(listener, app).await });

        let instance = Self {
            http: Client::new(),
            api_url,
            auth_url,
            admin: PgPoolOptions::new()
                .connect(&admin_url(&database))
                .await
                .unwrap(),
            api_db,
            database,
            auth_env,
            auth,
        };
        instance.wait_for_auth().await;
        instance
    }

    async fn wait_for_auth(&self) {
        for _ in 0..100 {
            let health = self.http.get(format!("{}/health", self.auth_url)).send();
            if health
                .await
                .is_ok_and(|response| response.status().is_success())
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("Auth did not answer /health within 10 seconds");
    }

    pub async fn close(mut self) {
        self.auth.kill().await.ok();
        self.admin.close().await;
        self.api_db.close().await;
        let server = PgPoolOptions::new()
            .max_connections(1)
            .connect(&admin_url("postgres"))
            .await
            .unwrap();
        sqlx::raw_sql(AssertSqlSafe(format!(
            "DROP DATABASE {} WITH (FORCE)",
            self.database
        )))
        .execute(&server)
        .await
        .unwrap();
        server.close().await;
    }

    /// Calls API as a client would, with a bearer token.
    pub async fn get(&self, path: &str, token: Option<&str>) -> Response {
        let mut request = self.http.get(format!("{}{path}", self.api_url));
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        request.send().await.unwrap()
    }

    pub async fn sign_up(&self, email: &str) -> User {
        let response = self
            .http
            .post(format!("{}/api/auth/sign-up/email", self.auth_url))
            .header("origin", WEB_URL)
            .json(&json!({ "email": email, "password": "correct horse battery", "name": email }))
            .send()
            .await
            .unwrap();
        let cookie = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|value| value.to_str().ok()?.split(';').next())
            .collect::<Vec<_>>()
            .join("; ");
        let body = ok_json(response).await;
        User {
            id: body["user"]["id"].as_str().unwrap().to_owned(),
            cookie,
        }
    }

    /// Calls Auth as Web's pages do: from Web's origin, with the session cookie.
    pub async fn auth_post(&self, user: &User, path: &str, body: Value) -> Value {
        let response = self
            .http
            .post(format!("{}/api/auth{path}", self.auth_url))
            .header("origin", WEB_URL)
            .header("cookie", &user.cookie)
            .json(&body)
            .send()
            .await
            .unwrap();
        ok_json(response).await
    }

    pub async fn session_token(&self, user: &User) -> String {
        let response = self
            .http
            .get(format!("{}/api/auth/token", self.auth_url))
            .header("cookie", &user.cookie)
            .send()
            .await
            .unwrap();
        ok_json(response).await["token"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    pub async fn create_organization(&self, owner: &User, slug: &str) -> String {
        let body = json!({ "name": slug, "slug": slug });
        let organization = self.auth_post(owner, "/organization/create", body).await;
        organization["id"].as_str().unwrap().to_owned()
    }

    pub async fn create_key(&self, owner: &User, organization_id: &str) -> Key {
        let body = json!({ "organizationId": organization_id, "name": "test" });
        let key = self.auth_post(owner, "/api-key/create", body).await;
        Key {
            id: key["id"].as_str().unwrap().to_owned(),
            secret: key["key"].as_str().unwrap().to_owned(),
        }
    }

    pub async fn key_token(&self, key: &Key) -> String {
        let response = self
            .http
            .post(format!("{}/api-key/token", self.auth_url))
            .header("x-api-key", &key.secret)
            .send()
            .await
            .unwrap();
        ok_json(response).await["token"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    /// Makes `invitee` a member with `role`, the way Auth allows: an invitation
    /// accepted by a user whose email is verified.
    pub async fn add_member(
        &self,
        owner: &User,
        organization_id: &str,
        invitee: &User,
        email: &str,
        role: &str,
    ) {
        sqlx::query(r#"UPDATE auth."user" SET "emailVerified" = true WHERE id = $1"#)
            .bind(&invitee.id)
            .execute(&self.admin)
            .await
            .unwrap();
        let body = json!({ "organizationId": organization_id, "email": email, "role": role });
        let invitation = self
            .auth_post(owner, "/organization/invite-member", body)
            .await;
        let body = json!({ "invitationId": invitation["id"] });
        self.auth_post(invitee, "/organization/accept-invitation", body)
            .await;
    }

    /// Signs a token with Auth's current signing key and an arbitrary payload,
    /// through Better Auth's server-only signJWT. Payload fields override
    /// the issuer, audience, and expiry Auth would set.
    pub async fn sign_token(&self, payload: Value) -> String {
        let output = node(&self.auth_env)
            .env("PAYLOAD", payload.to_string())
            .args(["--input-type=module", "-e", SIGN_TOKEN])
            .output()
            .await
            .expect("node runs");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
}

const SIGN_TOKEN: &str = r#"
import pg from "pg";
import { createAuth } from "./src/auth.ts";
import { readEnv } from "./src/env.ts";
const env = readEnv();
const pool = new pg.Pool({ connectionString: env.databaseUrl });
const payload = JSON.parse(process.env.PAYLOAD);
const { token } = await createAuth(pool, env).api.signJWT({ body: { payload } });
process.stdout.write(token);
await pool.end();
"#;

fn node(env: &[(&'static str, String)]) -> Command {
    let mut command = Command::new("node");
    command
        .current_dir(repo_root().join("apps/auth"))
        .envs(env.iter().map(|(name, value)| (*name, value.as_str())));
    command
}

pub async fn ok_json(response: Response) -> Value {
    let status = response.status();
    let text = response.text().await.unwrap();
    assert!(status.is_success(), "{status}: {text}");
    serde_json::from_str(&text).unwrap()
}

/// The status and error code of an API response.
pub async fn refusal(response: Response) -> (StatusCode, String) {
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    (status, body["code"].as_str().unwrap_or_default().to_owned())
}

pub struct User {
    pub id: String,
    pub cookie: String,
}

pub struct Key {
    pub id: String,
    pub secret: String,
}
