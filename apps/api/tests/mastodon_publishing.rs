//! Publishing to Mastodon, per 00004 D7 to D9, against a fake instance that
//! answers as Mastodon v4.5.20 does. The adapter is called directly, as a run's
//! commit step calls it.

mod support;

use std::sync::Arc;
use std::time::Duration;

use pipewhere_api::channels::Provider;
use pipewhere_api::credentials::Credentials;
use pipewhere_api::ids::new_id;
use pipewhere_api::mastodon::{Mastodon, MastodonAdapter, PUBLISH_LIMIT};
use pipewhere_api::runs::{Adapter, Commit, CommitResult};
use reqwest::StatusCode;
use support::mastodon::{FakeMastodon, STAN, link};
use support::{CREDENTIALS_KEY, Instance};

struct Linked {
    instance: Instance,
    fake: FakeMastodon,
    organization_id: String,
    channel_id: String,
    token: String,
}

impl Linked {
    async fn start() -> Self {
        let instance = Instance::start().await;
        let fake = FakeMastodon::start().await;
        let owner = instance.sign_up("owner@example.com").await;
        let organization_id = instance.create_organization(&owner, "acme").await;
        let (channel_id, token) = link(&instance.api_db, &organization_id, &fake, &STAN).await;
        Self {
            instance,
            fake,
            organization_id,
            channel_id,
            token,
        }
    }

    fn adapter(&self, limit: Duration) -> MastodonAdapter {
        MastodonAdapter {
            db: self.instance.api_db.clone(),
            credentials: Arc::new(Credentials::from_hex(CREDENTIALS_KEY).unwrap()),
            mastodon: Mastodon::http(),
            limit,
        }
    }

    async fn publish(&self, publication_id: &str) -> CommitResult {
        let commit = commit(&self.channel_id, publication_id);
        self.adapter(PUBLISH_LIMIT).commit(&commit).await
    }

    async fn channel_status(&self) -> String {
        sqlx::query_scalar("SELECT status FROM pipewhere.channel WHERE id = $1")
            .bind(&self.channel_id)
            .fetch_one(&self.instance.admin)
            .await
            .unwrap()
    }

    fn handle(&self) -> String {
        format!("@stan@{}", self.fake.host)
    }

    /// Points the channel at another host, as if its instance had moved there.
    async fn move_to(&self, host: &str) {
        sqlx::query(
            "INSERT INTO pipewhere.mastodon_instance (host, redirect_uri, client_id, client_secret)
             VALUES ($1, 'http://localhost:3000/connect/mastodon', 'client', '')",
        )
        .bind(host)
        .execute(&self.instance.admin)
        .await
        .unwrap();
        sqlx::query("UPDATE pipewhere.mastodon_channel SET host = $1")
            .bind(host)
            .execute(&self.instance.admin)
            .await
            .unwrap();
    }
}

fn commit(channel_id: &str, publication_id: &str) -> Commit {
    Commit {
        provider: Provider::Mastodon,
        body: "Hello from Pipewhere".to_owned(),
        publication_id: publication_id.to_owned(),
        channel_id: channel_id.to_owned(),
    }
}

fn reason(result: CommitResult) -> String {
    match result {
        CommitResult::Refused { reason } | CommitResult::Ambiguous { reason } => reason,
        CommitResult::Published { .. } => panic!("published"),
    }
}

#[tokio::test]
async fn publishing_posts_the_body_with_the_publication_as_its_idempotency_key() {
    let linked = Linked::start().await;
    let publication_id = new_id("publication");

    let result = linked.publish(&publication_id).await;
    assert_eq!(
        result,
        CommitResult::Published {
            remote_id: "11000".to_owned(),
            remote_url: Some(format!("http://{}/@stan/11000", linked.fake.host)),
        }
    );
    let statuses = linked.fake.statuses();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].account_id, STAN.id);
    assert_eq!(statuses[0].text, "Hello from Pipewhere");
    assert_eq!(statuses[0].idempotency_key, Some(publication_id));
    assert_eq!(linked.channel_status().await, "active");
    linked.instance.close().await;
}

/// 00004 D7: within its hour, a repeated key answers the status it first
/// posted. So a person's retry after an old call finished posts nothing new.
#[tokio::test]
async fn a_publication_committed_again_answers_its_first_status_and_posts_nothing() {
    let linked = Linked::start().await;
    let publication_id = new_id("publication");

    let first = linked.publish(&publication_id).await;
    let again = linked.publish(&publication_id).await;
    assert_eq!(first, again);
    assert_eq!(linked.fake.statuses().len(), 1);

    linked.publish(&new_id("publication")).await;
    assert_eq!(linked.fake.statuses().len(), 2);
    linked.instance.close().await;
}

#[tokio::test]
async fn a_revoked_token_refuses_and_marks_the_channel_revoked() {
    let linked = Linked::start().await;
    linked.fake.revoke(&linked.token);

    let result = linked.publish(&new_id("publication")).await;
    assert_eq!(
        result,
        CommitResult::Refused {
            reason: format!(
                "{} no longer accepts Pipewhere's access to {}, so nothing was posted. Connect the account again, then retry.",
                linked.fake.host,
                linked.handle()
            ),
        }
    );
    assert!(linked.fake.statuses().is_empty());
    assert_eq!(linked.channel_status().await, "access_revoked");
    linked.instance.close().await;
}

#[tokio::test]
async fn an_answer_that_shows_nothing_was_posted_refuses() {
    let linked = Linked::start().await;
    let host = linked.fake.host.clone();
    let cases = [
        (
            StatusCode::FORBIDDEN,
            r#"{"error":"Your login is currently disabled"}"#,
            format!("{host} refused the post with 403 Forbidden: Your login is currently disabled"),
        ),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            r#"{"error":"Validation failed: Text character limit of 500 exceeded"}"#,
            format!(
                "{host} refused the post with 422 Unprocessable Entity: Validation failed: Text character limit of 500 exceeded"
            ),
        ),
        (
            StatusCode::TOO_MANY_REQUESTS,
            r#"{"error":"Too many requests"}"#,
            format!("{host} refused the post with 429 Too Many Requests: Too many requests"),
        ),
        (
            StatusCode::BAD_REQUEST,
            "<html>Bad Request</html>",
            format!("{host} refused the post with 400 Bad Request."),
        ),
    ];
    for (status, body, expected) in cases {
        linked.fake.respond(status, body);
        let result = linked.publish(&new_id("publication")).await;
        assert_eq!(result, CommitResult::Refused { reason: expected });
    }
    assert_eq!(linked.channel_status().await, "active");
    linked.instance.close().await;
}

/// 00004 D7: a repeated key looks up the status it first posted, and
/// `ActiveRecord::RecordNotFound` answers 404 when that status was deleted.
#[tokio::test]
async fn a_publication_whose_post_was_deleted_says_when_to_retry() {
    let linked = Linked::start().await;
    linked
        .fake
        .respond(StatusCode::NOT_FOUND, r#"{"error":"Record not found"}"#);

    let result = linked.publish(&new_id("publication")).await;
    assert_eq!(
        result,
        CommitResult::Refused {
            reason: format!(
                "{} answered 404 Not Found, so nothing was posted. If the publication posted within the last hour and that post was deleted, Mastodon answers this way, so retry an hour after it posted.",
                linked.fake.host
            ),
        }
    );
    assert_eq!(linked.channel_status().await, "active");
    linked.instance.close().await;
}

/// Following a redirect to another host would drop the token, and that host's
/// 401 would revoke a working channel.
#[tokio::test]
async fn a_redirect_refuses_without_following_it() {
    let linked = Linked::start().await;
    let (_, port) = linked.fake.host.rsplit_once(':').unwrap();
    linked
        .fake
        .redirect(&format!("http://localhost:{port}/api/v1/statuses"));

    let result = linked.publish(&new_id("publication")).await;
    assert_eq!(
        result,
        CommitResult::Refused {
            reason: format!(
                "{} refused the post with 307 Temporary Redirect.",
                linked.fake.host
            ),
        }
    );
    assert_eq!(linked.fake.status_requests(), 1);
    assert_eq!(linked.channel_status().await, "active");
    linked.instance.close().await;
}

#[tokio::test]
async fn an_instances_error_is_kept_short_and_storable() {
    let linked = Linked::start().await;
    let long = format!(r#"{{"error":"a\u0000b{}"}}"#, "x".repeat(1000));
    linked.fake.respond(StatusCode::UNPROCESSABLE_ENTITY, &long);

    let reason = reason(linked.publish(&new_id("publication")).await);
    assert!(!reason.contains('\0'), "{reason}");
    assert!(reason.contains(": abxxx"), "{reason}");
    assert!(reason.ends_with("x…"), "{reason}");
    assert!(reason.chars().count() < 400, "{reason}");
    linked.instance.close().await;
}

/// 00002 D4: a server error, or an answer that cannot be read, does not prove
/// that nothing was posted.
#[tokio::test]
async fn an_answer_that_leaves_the_post_unknown_is_ambiguous() {
    let linked = Linked::start().await;
    let check = format!(
        "A post may exist, so check {} before resolving this publication.",
        linked.handle()
    );
    let host = linked.fake.host.clone();
    let cases = [
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "{}",
            format!("{host} answered 500 Internal Server Error. {check}"),
        ),
        (
            StatusCode::BAD_GATEWAY,
            "",
            format!("{host} answered 502 Bad Gateway. {check}"),
        ),
        (
            StatusCode::OK,
            "<html></html>",
            format!("{host} answered 200 OK with a body Pipewhere cannot read. {check}"),
        ),
        (
            StatusCode::OK,
            r#"{"id":" ","url":"https://mastodon.example/@stan/1"}"#,
            format!("{host} answered 200 OK without the post's id. {check}"),
        ),
    ];
    for (status, body, expected) in cases {
        linked.fake.respond(status, body);
        let result = linked.publish(&new_id("publication")).await;
        assert_eq!(result, CommitResult::Ambiguous { reason: expected });
    }
    assert_eq!(linked.channel_status().await, "active");
    linked.instance.close().await;
}

/// 00003 D18: a client shows the address as a link, so only http and https are
/// kept. The post exists, so its id is kept without one.
#[tokio::test]
async fn a_post_whose_address_is_not_http_keeps_its_id_without_the_address() {
    let linked = Linked::start().await;
    linked
        .fake
        .respond(StatusCode::OK, r#"{"id":"7","url":"javascript:alert(1)"}"#);
    assert_eq!(
        linked.publish(&new_id("publication")).await,
        CommitResult::Published {
            remote_id: "7".to_owned(),
            remote_url: None,
        }
    );
    linked
        .fake
        .respond(StatusCode::OK, r#"{"id":"8","url":null}"#);
    assert_eq!(
        linked.publish(&new_id("publication")).await,
        CommitResult::Published {
            remote_id: "8".to_owned(),
            remote_url: None,
        }
    );
    linked.instance.close().await;
}

/// 00003 D19: the call has a time limit, and a post may exist once it passes.
#[tokio::test]
async fn an_instance_that_answers_too_late_is_ambiguous() {
    let linked = Linked::start().await;
    linked.fake.post_then_hold();
    let commit = commit(&linked.channel_id, &new_id("publication"));

    let result = linked
        .adapter(Duration::from_millis(300))
        .commit(&commit)
        .await;
    assert_eq!(
        result,
        CommitResult::Ambiguous {
            reason: format!(
                "{} did not answer within 300ms. A post may exist, so check {} before resolving this publication.",
                linked.fake.host,
                linked.handle()
            ),
        }
    );
    assert_eq!(linked.fake.statuses().len(), 1);
    linked.instance.close().await;
}

#[tokio::test]
async fn an_instance_pipewhere_cannot_reach_refuses() {
    let linked = Linked::start().await;
    linked.move_to("127.0.0.1:1").await;

    let reason = reason(linked.publish(&new_id("publication")).await);
    assert!(
        reason.starts_with("Pipewhere could not reach 127.0.0.1:1, so nothing was posted: "),
        "{reason}"
    );
    assert!(reason.contains("Connection refused"), "{reason}");
    linked.instance.close().await;
}

/// 00004 D8: a connection that is never made sends nothing, even when the host
/// drops it rather than refusing it, as a firewall that drops packets does.
#[tokio::test]
async fn an_instance_that_never_answers_the_connection_refuses() {
    let linked = Linked::start().await;
    // A full accept queue makes the kernel drop each new connection silently.
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let listener = socket.listen(1).unwrap();
    let address = listener.local_addr().unwrap();
    let mut queued = Vec::new();
    loop {
        let connecting = tokio::net::TcpStream::connect(address);
        match tokio::time::timeout(Duration::from_millis(200), connecting).await {
            Ok(Ok(stream)) => queued.push(stream),
            Ok(Err(error)) => panic!("the full queue refused a connection: {error}"),
            Err(_) => break,
        }
        assert!(queued.len() < 64, "the accept queue never filled");
    }
    let host = address.to_string();
    linked.move_to(&host).await;

    // Shorter than the kernel's own wait for a connection, which on macOS
    // ends a loopback connection after about 8 seconds.
    let commit = commit(&linked.channel_id, &new_id("publication"));
    let reason = reason(linked.adapter(Duration::from_secs(5)).commit(&commit).await);
    assert!(
        reason.starts_with(&format!(
            "Pipewhere could not reach {host}, so nothing was posted: "
        )),
        "{reason}"
    );
    linked.instance.close().await;
}

#[tokio::test]
async fn a_channel_pipewhere_holds_no_access_to_refuses() {
    let linked = Linked::start().await;
    let channel_id = new_id("channel");
    sqlx::query(
        "INSERT INTO pipewhere.channel (id, organization_id, provider) VALUES ($1, $2, 'mastodon')",
    )
    .bind(&channel_id)
    .bind(&linked.organization_id)
    .execute(&linked.instance.admin)
    .await
    .unwrap();

    let result = linked
        .adapter(PUBLISH_LIMIT)
        .commit(&commit(&channel_id, &new_id("publication")))
        .await;
    assert_eq!(
        result,
        CommitResult::Refused {
            reason: "Pipewhere holds no access to this channel's account, so nothing was posted."
                .to_owned(),
        }
    );
    linked.instance.close().await;
}

#[tokio::test]
async fn a_token_sealed_under_another_key_refuses() {
    let linked = Linked::start().await;
    let lost = Credentials::from_hex(&"ab".repeat(32)).unwrap();
    sqlx::query("UPDATE pipewhere.mastodon_channel SET access_token = $1")
        .bind(lost.seal(&linked.channel_id, &linked.token))
        .execute(&linked.instance.admin)
        .await
        .unwrap();

    assert_eq!(
        linked.publish(&new_id("publication")).await,
        CommitResult::Refused {
            reason: "Pipewhere cannot read this channel's access under CREDENTIALS_KEY, so nothing was posted. Restore the key it was sealed under, or connect the account again."
                .to_owned(),
        }
    );
    assert!(linked.fake.statuses().is_empty());
    linked.instance.close().await;
}

#[tokio::test]
async fn a_threads_channel_cannot_publish_yet() {
    let linked = Linked::start().await;
    let mut threads = commit(&linked.channel_id, &new_id("publication"));
    threads.provider = Provider::Threads;

    assert_eq!(
        linked.adapter(PUBLISH_LIMIT).commit(&threads).await,
        CommitResult::Refused {
            reason: "Pipewhere cannot publish to Threads yet.".to_owned(),
        }
    );
    linked.instance.close().await;
}
