use std::io::IsTerminal;
use std::process::ExitCode;
use std::sync::Arc;

use pipewhere_api::credentials::Credentials;
use pipewhere_api::mastodon::{AccessChecks, Mastodon, MastodonAdapter, PUBLISH_LIMIT};
use pipewhere_worker::poller::Poller;
use pipewhere_worker::publication::Publication;
use pipewhere_worker::{Error, register};
use restate_sdk::prelude::*;
use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with_ansi(std::io::stdout().is_terminal())
        .init();
    // reqwest is built without a TLS provider, as in API, so the image does
    // not compile aws-lc.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("no other TLS provider is installed");

    let result = match std::env::args().nth(1).as_deref() {
        None | Some("serve") => serve().await,
        Some("register") => register_worker().await,
        Some("health") => health().await,
        Some(other) => {
            Err(format!("unknown command {other:?}; expected serve, register, or health").into())
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn serve() -> Result<(), Error> {
    let db = PgPoolOptions::new()
        .connect(&required("DATABASE_URL")?)
        .await?;
    let credentials = Arc::new(Credentials::from_hex(&required("CREDENTIALS_KEY")?)?);
    let poller = Poller::new(db.clone(), required("RESTATE_INGRESS_URL")?)?;
    let checks = AccessChecks {
        db: db.clone(),
        credentials: credentials.clone(),
        mastodon: Mastodon::https(),
    };
    let listener = TcpListener::bind(("0.0.0.0", port()?)).await?;
    let endpoint = Endpoint::builder()
        .bind(Publication {
            db: db.clone(),
            adapter: Arc::new(MastodonAdapter {
                db,
                credentials,
                mastodon: Mastodon::https(),
                limit: PUBLISH_LIMIT,
            }),
        })
        .build();
    tokio::select! {
        () = HttpServer::new(endpoint).serve_with_cancel(listener, shutdown()) => {}
        () = poller.run() => {}
        () = checks.run() => {}
    }
    Ok(())
}

fn port() -> Result<u16, Error> {
    Ok(std::env::var("PORT").map_or(Ok(9080), |port| port.parse())?)
}

// The endpoint speaks HTTP/2 only, and it logs a warning for every connection
// that closes before sending a request, so a bare TCP check would fill the log.
async fn health() -> Result<(), Error> {
    reqwest::Client::builder()
        .http2_prior_knowledge()
        .build()?
        .get(format!("http://127.0.0.1:{}/health", port()?))
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

async fn register_worker() -> Result<(), Error> {
    register(&required("RESTATE_ADMIN_URL")?, &required("WORKER_URL")?).await
}

// Docker stops a container with SIGTERM, which a process running as PID 1
// otherwise ignores until Docker kills it. The SDK's own listen_and_serve stops
// only on Ctrl-C.
async fn shutdown() {
    let mut terminate = signal(SignalKind::terminate()).expect("SIGTERM handler installs");
    tokio::select! {
        _ = terminate.recv() => {}
        _ = tokio::signal::ctrl_c() => {}
    }
}

fn required(name: &str) -> Result<String, Error> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{name} is not set. See compose.yaml.").into())
}
