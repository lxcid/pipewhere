use std::time::Duration;

use sqlx::PgPool;
use tokio::time::{MissedTickBehavior, interval};

use crate::Error;

const TICK: Duration = Duration::from_secs(5);

/// Finds due work in PostgreSQL and sends each publication's run to Restate.
pub struct Poller {
    db: PgPool,
    http: reqwest::Client,
    ingress_url: String,
}

impl Poller {
    pub fn new(db: PgPool, ingress_url: String) -> Result<Self, Error> {
        // A Restate that accepts a connection and never answers would otherwise
        // stop every later tick without an error.
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?;
        Ok(Self {
            db,
            http,
            ingress_url,
        })
    }

    pub async fn run(&self) {
        let mut ticks = interval(TICK);
        ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticks.tick().await;
            if let Err(error) = self.tick().await {
                tracing::error!("a poller tick failed: {error}");
            }
        }
    }

    /// Stops at the first send that fails. The next tick starts again from the
    /// oldest work, which Restate is unlikely to refuse for itself alone.
    pub async fn tick(&self) -> Result<(), Error> {
        for publication_id in self.due().await? {
            self.send_run(&publication_id).await?;
        }
        Ok(())
    }

    pub async fn due(&self) -> Result<Vec<String>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT id FROM pipewhere.publication
             WHERE state = 'scheduled' AND planned_at <= now()
             ORDER BY planned_at
             LIMIT 100",
        )
        .fetch_all(&self.db)
        .await
    }

    pub async fn send_run(&self, publication_id: &str) -> Result<(), Error> {
        self.http
            .post(format!(
                "{}/Publication/{publication_id}/run/send",
                self.ingress_url
            ))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
}
