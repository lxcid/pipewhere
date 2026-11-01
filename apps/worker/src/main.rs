use std::io::IsTerminal;
use std::process::ExitCode;

use restate_sdk::prelude::*;
use serde_json::{Value, json};
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
        Some("register") => register().await,
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

type Error = Box<dyn std::error::Error>;

async fn serve() -> Result<(), Error> {
    let listener = TcpListener::bind(("0.0.0.0", port()?)).await?;
    HttpServer::new(Endpoint::builder().build())
        .serve_with_cancel(listener, shutdown())
        .await;
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

// Restate keeps a deployment's handlers when its URI is registered again, even
// after they changed, unless the registration forces it. Forcing replaces them
// under the same deployment, so an invocation in flight resumes on the new code.
async fn register() -> Result<(), Error> {
    let admin_url = required("RESTATE_ADMIN_URL")?;
    let worker_url = required("WORKER_URL")?;
    let response = reqwest::Client::new()
        .post(format!("{admin_url}/deployments"))
        .json(&json!({ "uri": worker_url, "force": true }))
        .send()
        .await
        .map_err(|error| format!("Restate's admin API at {admin_url} is unreachable: {error}"))?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Err(format!("Restate refused to register {worker_url}: {status} {body}").into());
    }
    let deployment: Value = serde_json::from_str(&body)?;
    tracing::info!(
        id = %deployment["id"].as_str().unwrap_or_default(),
        services = %deployment["services"].as_array().map_or(0, Vec::len),
        "registered {worker_url} with Restate"
    );
    Ok(())
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
