use std::io::IsTerminal;
use std::process::ExitCode;
use std::sync::Arc;

use pipewhere_api::tokens::TokenVerifier;
use pipewhere_api::{AppState, migrate, router};
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

    let result = match std::env::args().nth(1).as_deref() {
        None | Some("serve") => serve().await,
        Some("migrate") => run_migrations().await,
        Some(other) => Err(format!("unknown command {other:?}; expected serve or migrate").into()),
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
    let db = PgPoolOptions::new()
        .connect(&required("DATABASE_URL")?)
        .await?;
    let tokens = TokenVerifier::new(&required("AUTH_URL")?, required("AUTH_JWKS_URL")?);
    let port = std::env::var("PORT").unwrap_or_else(|_| "3002".into());
    let listener = TcpListener::bind(("0.0.0.0", port.parse::<u16>()?)).await?;
    tracing::info!("api listening on {port}");
    let app = router(AppState {
        db,
        tokens: Arc::new(tokens),
    });
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn run_migrations() -> Result<(), Error> {
    let db = PgPoolOptions::new()
        .max_connections(1)
        .connect(&required("DATABASE_URL")?)
        .await?;
    migrate(&db).await?;
    tracing::info!("api migrations applied");
    Ok(())
}

// Docker stops a container with SIGTERM, which a process running as PID 1
// otherwise ignores until Docker kills it.
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
        .ok_or_else(|| format!("{name} is not set. See .env.example.").into())
}
