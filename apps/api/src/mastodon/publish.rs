//! Publishing to Mastodon, per 00004 D7 to D9.

use std::sync::Arc;
use std::time::Duration;

use reqwest::{StatusCode, Url};
use serde::Deserialize;
use sqlx::PgPool;

use super::Mastodon;
use crate::channels::Provider;
use crate::credentials::Credentials;
use crate::runs::{Adapter, Commit, CommitResult, NoAdapter};

/// How long a post may take before its outcome is unknown, per 00003 D19.
/// Restate asks an invocation that stays silent for a minute to suspend, so a
/// call ends well within that and its answer is recorded first.
pub const PUBLISH_LIMIT: Duration = Duration::from_secs(30);

// An instance's error is shown to the operator as part of a reason.
const ERROR_CHARACTERS: usize = 300;

pub struct MastodonAdapter {
    pub db: PgPool,
    pub credentials: Arc<Credentials>,
    pub mastodon: Mastodon,
    pub limit: Duration,
}

#[derive(sqlx::FromRow)]
struct Access {
    host: String,
    handle: String,
    access_token: Vec<u8>,
}

#[derive(Deserialize)]
struct Status {
    id: String,
    url: Option<String>,
}

#[derive(Deserialize)]
struct Error {
    error: String,
}

impl Adapter for MastodonAdapter {
    async fn commit(&self, commit: &Commit) -> CommitResult {
        if commit.provider != Provider::Mastodon {
            return NoAdapter.commit(commit).await;
        }
        let access = sqlx::query_as::<_, Access>(
            "SELECT m.host, COALESCE(c.handle, 'the account') AS handle, m.access_token
             FROM pipewhere.mastodon_channel m
             JOIN pipewhere.channel c ON c.id = m.channel_id
             WHERE m.channel_id = $1",
        )
        .bind(&commit.channel_id)
        .fetch_optional(&self.db)
        .await;
        let access = match access {
            Ok(Some(access)) => access,
            Ok(None) => {
                return refused(
                    "Pipewhere holds no access to this channel's account, so nothing was posted.",
                );
            }
            Err(error) => {
                return refused(format!(
                    "Pipewhere could not read this channel's access, so nothing was posted: {error}"
                ));
            }
        };
        let Ok(token) = self
            .credentials
            .open(&commit.channel_id, &access.access_token)
        else {
            tracing::error!(
                channel_id = commit.channel_id,
                "a channel's token does not open under CREDENTIALS_KEY"
            );
            return refused(
                "Pipewhere cannot read this channel's access under CREDENTIALS_KEY, so nothing was posted. Restore the key it was sealed under, or connect the account again.",
            );
        };

        let Access { host, handle, .. } = &access;
        let check =
            format!("A post may exist, so check {handle} before resolving this publication.");
        // Mastodon answers a repeated key with the status it first posted,
        // per 00004 D7, so a publication keeps one key across its runs.
        let response = self
            .mastodon
            .http
            .post(self.mastodon.url(host, "/api/v1/statuses"))
            .bearer_auth(&token)
            .header("Idempotency-Key", &commit.publication_id)
            .form(&[("status", &commit.body)])
            .timeout(self.limit)
            .send()
            .await;
        let response = match response {
            Ok(response) => response,
            Err(error) if error.is_connect() => {
                return refused(format!(
                    "Pipewhere could not reach {host}, so nothing was posted: {}",
                    described(&error)
                ));
            }
            Err(error) if error.is_timeout() => {
                return ambiguous(format!(
                    "{host} did not answer within {:?}. {check}",
                    self.limit
                ));
            }
            Err(error) => {
                return ambiguous(format!(
                    "{host} did not answer: {}. {check}",
                    described(&error)
                ));
            }
        };

        let status = response.status();
        if status == StatusCode::UNAUTHORIZED {
            self.revoke(&commit.channel_id).await;
            return refused(format!(
                "{host} no longer accepts Pipewhere's access to {handle}, so nothing was posted. Connect the account again, then retry."
            ));
        }
        // 00002 D4: a server error does not show that nothing was posted.
        if status.is_server_error() {
            return ambiguous(format!("{host} answered {status}. {check}"));
        }
        // A repeated key whose post was deleted finds no status, per D7.
        if status == StatusCode::NOT_FOUND {
            return refused(format!(
                "{host} answered {status}, so nothing was posted. If the publication posted within the last hour and that post was deleted, Mastodon answers this way, so retry an hour after it posted."
            ));
        }
        let body = match response.bytes().await {
            Ok(body) => body,
            Err(error) if error.is_timeout() && status.is_success() => {
                return ambiguous(format!(
                    "{host} did not answer within {:?}. {check}",
                    self.limit
                ));
            }
            Err(error) if status.is_success() => {
                return ambiguous(format!(
                    "{host} did not answer: {}. {check}",
                    described(&error)
                ));
            }
            Err(_) => Default::default(),
        };
        if !status.is_success() {
            return refused(match serde_json::from_slice::<Error>(&body) {
                Ok(Error { error }) => format!(
                    "{host} refused the post with {status}: {}",
                    shortened(&error)
                ),
                Err(_) => format!("{host} refused the post with {status}."),
            });
        }
        let Ok(posted) = serde_json::from_slice::<Status>(&body) else {
            return ambiguous(format!(
                "{host} answered {status} with a body Pipewhere cannot read. {check}"
            ));
        };
        // 00003 D18. Refusing the answer would lose the record of a post that
        // exists, so a post keeps its id and only drops an unsafe address.
        if posted.id.trim().is_empty() || posted.id.contains('\0') {
            return ambiguous(format!(
                "{host} answered {status} without the post's id. {check}"
            ));
        }
        let remote_url = posted.url.and_then(|text| match Url::parse(&text) {
            Ok(url) if matches!(url.scheme(), "http" | "https") => Some(String::from(url)),
            _ => {
                tracing::warn!(
                    channel_id = commit.channel_id,
                    remote_id = posted.id,
                    "{host} answered a post address that is not http or https, so it is dropped"
                );
                None
            }
        });
        CommitResult::Published {
            remote_id: posted.id,
            remote_url,
        }
    }
}

impl MastodonAdapter {
    /// Shows that the channel cannot publish, per 00004 D9. Failing to record
    /// it leaves the publication's reason as the only sign, so it is logged.
    async fn revoke(&self, channel_id: &str) {
        let revoked =
            sqlx::query("UPDATE pipewhere.channel SET status = 'access_revoked' WHERE id = $1")
                .bind(channel_id)
                .execute(&self.db)
                .await;
        if let Err(error) = revoked {
            tracing::error!(
                channel_id,
                "a revoked channel's status was not recorded: {error}"
            );
        }
    }
}

fn refused(reason: impl Into<String>) -> CommitResult {
    CommitResult::Refused {
        reason: reason.into(),
    }
}

fn ambiguous(reason: impl Into<String>) -> CommitResult {
    CommitResult::Ambiguous {
        reason: reason.into(),
    }
}

/// A failed request with its causes. reqwest's own text names the request,
/// not why it failed, such as a refused connection or a bad certificate.
fn described(error: &reqwest::Error) -> String {
    let mut text = error.to_string();
    let mut cause = std::error::Error::source(error);
    while let Some(error) = cause {
        text.push_str(": ");
        text.push_str(&error.to_string());
        cause = error.source();
    }
    text
}

/// An instance's error, as a reason can hold it. PostgreSQL text cannot hold
/// U+0000, and a reason that cannot be stored leaves the post's outcome
/// unrecorded.
fn shortened(error: &str) -> String {
    let error: String = error.chars().filter(|&c| c != '\0').collect();
    if error.chars().count() <= ERROR_CHARACTERS {
        return error;
    }
    let mut short: String = error.chars().take(ERROR_CHARACTERS).collect();
    short.push('…');
    short
}
