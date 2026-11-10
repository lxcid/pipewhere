//! Checking each Mastodon channel's access, per 00004 D11 and D12.

use std::sync::Arc;
use std::time::Duration;

use reqwest::StatusCode;
use sqlx::PgPool;
use tokio::time::{MissedTickBehavior, interval};

use super::{InstanceError, Mastodon, described};
use crate::credentials::Credentials;

/// How often each channel is checked, per 00004 D11.
const ACCESS_CHECK_INTERVAL: Duration = Duration::from_secs(5 * 60);

pub struct AccessChecks {
    pub db: PgPool,
    pub credentials: Arc<Credentials>,
    pub mastodon: Mastodon,
}

#[derive(sqlx::FromRow)]
struct Checked {
    channel_id: String,
    host: String,
    access_token: Vec<u8>,
}

impl AccessChecks {
    pub async fn run(&self) {
        let mut ticks = interval(ACCESS_CHECK_INTERVAL);
        ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticks.tick().await;
            if let Err(error) = self.check_all().await {
                tracing::error!("the channels to check could not be read: {error}");
            }
        }
    }

    /// Checks every channel that can publish, or that a check may let publish
    /// again. A revoked token never works again, so only reconnecting clears
    /// `access_revoked`.
    pub async fn check_all(&self) -> Result<(), sqlx::Error> {
        let channels = sqlx::query_as::<_, Checked>(
            "SELECT m.channel_id, m.host, m.access_token
             FROM pipewhere.mastodon_channel m
             JOIN pipewhere.channel c ON c.id = m.channel_id
             WHERE c.status IN ('active', 'account_unavailable')
             ORDER BY m.channel_id",
        )
        .fetch_all(&self.db)
        .await?;
        for channel in channels {
            self.check(&channel).await;
        }
        Ok(())
    }

    async fn check(&self, channel: &Checked) {
        let Checked {
            channel_id, host, ..
        } = channel;
        let Ok(token) = self.credentials.open(channel_id, &channel.access_token) else {
            tracing::error!(
                channel_id,
                "a channel's token does not open under CREDENTIALS_KEY, so its access is not checked"
            );
            return;
        };
        let response = self
            .mastodon
            .http
            .get(
                self.mastodon
                    .url(host, "/api/v1/accounts/verify_credentials"),
            )
            .bearer_auth(&token)
            .send()
            .await;
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                tracing::warn!(channel_id, "{host} was not checked: {}", described(&error));
                return;
            }
        };
        let status = response.status();
        if status.is_success() {
            make_available(&self.db, channel_id).await;
            return;
        }
        if status == StatusCode::UNAUTHORIZED {
            revoke(&self.db, channel_id).await;
            return;
        }
        // A proxy in front of the instance can answer 403 too. Only Mastodon's
        // own answer says anything about the account.
        let body = response.bytes().await.unwrap_or_default();
        match serde_json::from_slice::<InstanceError>(&body) {
            Ok(InstanceError { error }) if status == StatusCode::FORBIDDEN => {
                tracing::warn!(
                    channel_id,
                    "{host} reports the account unavailable: {error}"
                );
                make_unavailable(&self.db, channel_id).await;
            }
            _ => tracing::warn!(channel_id, "{host} answered a check with {status}"),
        }
    }
}

/// Records that the instance no longer accepts the channel's token, per D9.
pub(super) async fn revoke(db: &PgPool, channel_id: &str) {
    set_status(
        db,
        channel_id,
        "access_revoked",
        &["active", "account_unavailable"],
    )
    .await;
}

/// Records that the instance reports the account unavailable, per D12.
pub(super) async fn make_unavailable(db: &PgPool, channel_id: &str) {
    set_status(db, channel_id, "account_unavailable", &["active"]).await;
}

async fn make_available(db: &PgPool, channel_id: &str) {
    set_status(db, channel_id, "active", &["account_unavailable"]).await;
}

/// Changes the channel's status only from one of `from`, so nothing but
/// reconnecting clears `access_revoked`. A failure is logged, since the status
/// is then the only thing left wrong.
async fn set_status(db: &PgPool, channel_id: &str, to: &str, from: &[&str]) {
    let set =
        sqlx::query("UPDATE pipewhere.channel SET status = $2 WHERE id = $1 AND status = ANY($3)")
            .bind(channel_id)
            .bind(to)
            .bind(from)
            .execute(db)
            .await;
    if let Err(error) = set {
        tracing::error!(
            channel_id,
            "a channel's status was not set to {to}: {error}"
        );
    }
}
