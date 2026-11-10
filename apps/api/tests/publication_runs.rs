//! A publication's run, per 00003 D2, D6, D7, and D8, and 00002 D3, D4, D16, and
//! D19. These tests take the steps Worker will take, against PostgreSQL and a
//! fake adapter, without Restate.

mod support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use pipewhere_api::channels::Provider;
use pipewhere_api::ids::new_id;
use pipewhere_api::publications::PublicationState::{
    self, Cancelled, Failed, OutcomeUnknown, Preparing, Published, Publishing, Scheduled,
};
use pipewhere_api::runs::{Adapter, Commit, CommitResult, NoAdapter, step};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Instance, ok_json, refusal};
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;

const MISSED_ITS_TIME: &str =
    "Missed its time: its provider commit had not started by its deadline.";

#[derive(Clone)]
enum Answer {
    Published,
    PublishedWithoutAddress,
    Refused,
    Ambiguous,
}

/// Answers every commit the same way, and can hold a commit until released.
struct FakeAdapter {
    answer: Answer,
    seen: Mutex<Vec<(Provider, String)>>,
    calls: AtomicUsize,
    entered: Semaphore,
    gate: Option<Semaphore>,
}

impl FakeAdapter {
    fn answering(answer: Answer) -> Arc<Self> {
        Arc::new(Self {
            answer,
            seen: Mutex::new(Vec::new()),
            calls: AtomicUsize::new(0),
            entered: Semaphore::new(0),
            gate: None,
        })
    }

    fn holding(answer: Answer) -> Arc<Self> {
        Arc::new(Self {
            answer,
            seen: Mutex::new(Vec::new()),
            calls: AtomicUsize::new(0),
            entered: Semaphore::new(0),
            gate: Some(Semaphore::new(0)),
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    async fn wait_until_called(&self) {
        tokio::time::timeout(Duration::from_secs(10), self.entered.acquire())
            .await
            .expect("the adapter is called within 10 seconds")
            .unwrap()
            .forget();
    }

    fn release(&self) {
        self.gate.as_ref().unwrap().add_permits(1);
    }
}

impl Adapter for FakeAdapter {
    async fn commit(&self, commit: &Commit) -> CommitResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.seen
            .lock()
            .unwrap()
            .push((commit.provider, commit.body.clone()));
        self.entered.add_permits(1);
        // Only a test that expects a call releases it, so a call it did not
        // expect fails here rather than hanging.
        if let Some(gate) = &self.gate {
            tokio::time::timeout(Duration::from_secs(10), gate.acquire())
                .await
                .expect("the commit is released within 10 seconds")
                .unwrap()
                .forget();
        }
        match self.answer {
            Answer::Published => CommitResult::Published {
                remote_id: "109".to_owned(),
                remote_url: Some("https://mastodon.example/@a/109".to_owned()),
            },
            Answer::PublishedWithoutAddress => CommitResult::Published {
                remote_id: "109".to_owned(),
                remote_url: None,
            },
            Answer::Refused => CommitResult::Refused {
                reason: "Mastodon refused the post: too long".to_owned(),
            },
            Answer::Ambiguous => CommitResult::Ambiguous {
                reason: "Mastodon did not answer within 30 seconds".to_owned(),
            },
        }
    }
}

fn run_step(db: &PgPool, adapter: &Arc<FakeAdapter>, id: &str) -> JoinHandle<PublicationState> {
    let (db, adapter, id) = (db.clone(), adapter.clone(), id.to_owned());
    tokio::spawn(async move { step(&db, &*adapter, &id).await.unwrap() })
}

/// A publication that says "Copy", on a new Mastodon channel, with a 10-minute
/// tolerance. `planned` is an interval from now. A `preparing` or `publishing`
/// publication gets a running attempt, as a claim would have left it.
async fn insert_publication(
    db: &PgPool,
    organization_id: &str,
    state: PublicationState,
    planned: &str,
) -> String {
    let channel_id = new_id("channel");
    let content_id = new_id("content");
    let id = new_id("publication");
    sqlx::query(
        "INSERT INTO pipewhere.channel (id, organization_id, provider) VALUES ($1, $2, 'mastodon')",
    )
    .bind(&channel_id)
    .bind(organization_id)
    .execute(db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO pipewhere.content (id, organization_id, body) VALUES ($1, $2, 'Source')",
    )
    .bind(&content_id)
    .bind(organization_id)
    .execute(db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO pipewhere.publication
             (id, organization_id, channel_id, content_id, body, state, planned_at, tolerance_seconds,
              remote_id)
         VALUES ($1, $2, $3, $4, 'Copy', $5, now() + $6::text::interval, 600,
             CASE WHEN $5 = 'published' THEN 'earlier' END)",
    )
    .bind(&id)
    .bind(organization_id)
    .bind(&channel_id)
    .bind(&content_id)
    .bind(state)
    .bind(planned)
    .execute(db)
    .await
    .unwrap();
    if matches!(state, Preparing | Publishing) {
        insert_running_attempt(db, &id).await;
    }
    id
}

async fn insert_running_attempt(db: &PgPool, publication_id: &str) {
    sqlx::query(
        "INSERT INTO pipewhere.publication_attempt (id, publication_id, started_at)
         VALUES ($1, $2, now())",
    )
    .bind(new_id("publication_attempt"))
    .bind(publication_id)
    .execute(db)
    .await
    .unwrap();
}

async fn state_of(db: &PgPool, id: &str) -> PublicationState {
    sqlx::query_scalar("SELECT state FROM pipewhere.publication WHERE id = $1")
        .bind(id)
        .fetch_one(db)
        .await
        .unwrap()
}

#[derive(Debug, PartialEq, sqlx::FromRow)]
struct Attempt {
    result: Option<String>,
    reason: Option<String>,
    started_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
}

/// Oldest first.
async fn attempts_of(db: &PgPool, id: &str) -> Vec<Attempt> {
    sqlx::query_as(
        "SELECT result, reason, started_at, ended_at FROM pipewhere.publication_attempt
         WHERE publication_id = $1 ORDER BY id",
    )
    .bind(id)
    .fetch_all(db)
    .await
    .unwrap()
}

/// The result and reason of the publication's only attempt.
async fn only_attempt(db: &PgPool, id: &str) -> (Option<String>, Option<String>) {
    let mut attempts = attempts_of(db, id).await;
    assert_eq!(attempts.len(), 1, "{attempts:?}");
    let attempt = attempts.remove(0);
    assert_eq!(
        attempt.result.is_some(),
        attempt.ended_at.is_some(),
        "{attempt:?}"
    );
    (attempt.result, attempt.reason)
}

async fn remote_of(db: &PgPool, id: &str) -> (Option<String>, Option<String>) {
    sqlx::query_as("SELECT remote_id, remote_url FROM pipewhere.publication WHERE id = $1")
        .bind(id)
        .fetch_one(db)
        .await
        .unwrap()
}

/// Waits until `count` statements in the test's database wait on a lock.
async fn wait_for_blocked(instance: &Instance, count: i64) {
    for _ in 0..500 {
        let blocked: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity
             WHERE datname = current_database() AND wait_event_type = 'Lock'",
        )
        .fetch_one(&instance.admin)
        .await
        .unwrap();
        if blocked == count {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{count} statements did not start waiting on a lock");
}

fn publication_path(organization_id: &str, id: &str) -> String {
    format!("/v1/orgs/{organization_id}/publications/{id}")
}

async fn set_up(instance: &Instance) -> (String, String) {
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;
    (organization_id, token)
}

#[tokio::test]
async fn a_claim_moves_a_due_publication_to_preparing_with_a_running_attempt() {
    let instance = Instance::start().await;
    let (organization_id, token) = set_up(&instance).await;
    let db = &instance.api_db;
    let adapter = FakeAdapter::answering(Answer::Published);

    // Within the tolerance, by how much it was planned ahead of the deadline.
    for (planned, tolerance) in [
        ("-1 minute", 600),
        ("-9 minutes", 600),
        ("-4 minutes", 300),
        ("-23 hours", 86_400),
    ] {
        let id = insert_publication(db, &organization_id, Scheduled, planned).await;
        sqlx::query("UPDATE pipewhere.publication SET tolerance_seconds = $2 WHERE id = $1")
            .bind(&id)
            .bind(tolerance)
            .execute(db)
            .await
            .unwrap();
        assert_eq!(step(db, &*adapter, &id).await.unwrap(), Preparing);
        assert_eq!(state_of(db, &id).await, Preparing, "{planned}");
        assert_eq!(only_attempt(db, &id).await, (None, None), "{planned}");
        assert!(attempts_of(db, &id).await[0].ended_at.is_none());
    }
    assert_eq!(adapter.calls(), 0, "a claim calls no provider");

    // A client reads the running attempt.
    let id = insert_publication(db, &organization_id, Scheduled, "-1 minute").await;
    step(db, &*adapter, &id).await.unwrap();
    let path = format!("{}/attempts", publication_path(&organization_id, &id));
    let attempts = ok_json(instance.get(&path, Some(&token)).await).await;
    assert_eq!(attempts["attempts"][0]["result"], Value::Null);
    assert_eq!(attempts["attempts"][0]["publication_id"], id.as_str());
    instance.close().await;
}

#[tokio::test]
async fn a_claim_past_the_deadline_fails_with_an_attempt_that_starts_and_ends_at_once() {
    let instance = Instance::start().await;
    let (organization_id, token) = set_up(&instance).await;
    let db = &instance.api_db;
    let adapter = FakeAdapter::answering(Answer::Published);

    for (planned, tolerance) in [
        ("-11 minutes", 600),
        ("-6 minutes", 300),
        ("-25 hours", 86_400),
    ] {
        let id = insert_publication(db, &organization_id, Scheduled, planned).await;
        sqlx::query("UPDATE pipewhere.publication SET tolerance_seconds = $2 WHERE id = $1")
            .bind(&id)
            .bind(tolerance)
            .execute(db)
            .await
            .unwrap();
        assert_eq!(step(db, &*adapter, &id).await.unwrap(), Failed);
        assert_eq!(state_of(db, &id).await, Failed, "{planned}");
        assert_eq!(
            only_attempt(db, &id).await,
            (Some("failed".to_owned()), Some(MISSED_ITS_TIME.to_owned())),
            "{planned}"
        );
        let attempt = &attempts_of(db, &id).await[0];
        assert_eq!(Some(attempt.started_at), attempt.ended_at, "{planned}");
    }
    assert_eq!(adapter.calls(), 0);

    // An explicit retry starts a new run, with a new attempt.
    let id = insert_publication(db, &organization_id, Scheduled, "-1 hour").await;
    assert_eq!(step(db, &*adapter, &id).await.unwrap(), Failed);
    let response = instance
        .request(
            Method::POST,
            &format!("{}/retry", publication_path(&organization_id, &id)),
            Some(&token),
        )
        .header("idempotency-key", uuid::Uuid::now_v7().simple().to_string())
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(ok_json(response).await["state"], "scheduled");
    assert_eq!(step(db, &*adapter, &id).await.unwrap(), Preparing);
    assert_eq!(step(db, &*adapter, &id).await.unwrap(), Published);
    let attempts = attempts_of(db, &id).await;
    assert_eq!(attempts.len(), 2, "{attempts:?}");
    assert_eq!(attempts[0].result.as_deref(), Some("failed"));
    assert_eq!(attempts[1].result.as_deref(), Some("published"));
    assert_eq!(adapter.calls(), 1);
    instance.close().await;
}

// A run can outlive the time it was sent for. It must not publish early.
#[tokio::test]
async fn a_run_stops_at_a_publication_whose_time_has_not_come() {
    let instance = Instance::start().await;
    let (organization_id, _) = set_up(&instance).await;
    let db = &instance.api_db;
    let adapter = FakeAdapter::answering(Answer::Published);

    let id = insert_publication(db, &organization_id, Scheduled, "1 hour").await;
    assert_eq!(step(db, &*adapter, &id).await.unwrap(), Scheduled);
    assert_eq!(state_of(db, &id).await, Scheduled);
    assert_eq!(attempts_of(db, &id).await, []);
    assert_eq!(adapter.calls(), 0);
    instance.close().await;
}

#[tokio::test]
async fn a_run_stops_at_a_publication_it_cannot_continue() {
    let instance = Instance::start().await;
    let (organization_id, _) = set_up(&instance).await;
    let db = &instance.api_db;
    let adapter = FakeAdapter::answering(Answer::Published);

    for state in [Published, Failed, OutcomeUnknown] {
        let id = insert_publication(db, &organization_id, state, "-1 minute").await;
        assert_eq!(step(db, &*adapter, &id).await.unwrap(), state);
        assert_eq!(state_of(db, &id).await, state);
        assert_eq!(attempts_of(db, &id).await, [], "{state:?}");
    }
    assert_eq!(adapter.calls(), 0);

    // A publication that does not exist is an error, not a state to stop at.
    assert!(matches!(
        step(db, &*adapter, &new_id("publication")).await,
        Err(sqlx::Error::RowNotFound)
    ));
    instance.close().await;
}

#[tokio::test]
async fn a_commit_records_what_the_provider_answered() {
    let instance = Instance::start().await;
    let (organization_id, token) = set_up(&instance).await;
    let db = &instance.api_db;

    let adapter = FakeAdapter::answering(Answer::Published);
    let id = insert_publication(db, &organization_id, Preparing, "-1 minute").await;
    assert_eq!(step(db, &*adapter, &id).await.unwrap(), Published);
    assert_eq!(state_of(db, &id).await, Published);
    assert_eq!(
        remote_of(db, &id).await,
        (
            Some("109".to_owned()),
            Some("https://mastodon.example/@a/109".to_owned())
        )
    );
    assert_eq!(
        only_attempt(db, &id).await,
        (Some("published".to_owned()), None)
    );
    assert_eq!(
        *adapter.seen.lock().unwrap(),
        [(Provider::Mastodon, "Copy".to_owned())]
    );
    // The next invocation changes nothing and calls no provider.
    let ended = attempts_of(db, &id).await;
    assert_eq!(step(db, &*adapter, &id).await.unwrap(), Published);
    assert_eq!(attempts_of(db, &id).await, ended);
    assert_eq!(adapter.calls(), 1);
    let read = ok_json(
        instance
            .get(&publication_path(&organization_id, &id), Some(&token))
            .await,
    )
    .await;
    assert_eq!(read["state"], "published");
    assert_eq!(read["remote_id"], "109");

    let adapter = FakeAdapter::answering(Answer::PublishedWithoutAddress);
    let id = insert_publication(db, &organization_id, Preparing, "-1 minute").await;
    assert_eq!(step(db, &*adapter, &id).await.unwrap(), Published);
    assert_eq!(remote_of(db, &id).await, (Some("109".to_owned()), None));

    let adapter = FakeAdapter::answering(Answer::Refused);
    let id = insert_publication(db, &organization_id, Preparing, "-1 minute").await;
    assert_eq!(step(db, &*adapter, &id).await.unwrap(), Failed);
    assert_eq!(state_of(db, &id).await, Failed);
    assert_eq!(remote_of(db, &id).await, (None, None));
    assert_eq!(
        only_attempt(db, &id).await,
        (
            Some("failed".to_owned()),
            Some("Mastodon refused the post: too long".to_owned())
        )
    );

    let adapter = FakeAdapter::answering(Answer::Ambiguous);
    let id = insert_publication(db, &organization_id, Preparing, "-1 minute").await;
    assert_eq!(step(db, &*adapter, &id).await.unwrap(), OutcomeUnknown);
    assert_eq!(state_of(db, &id).await, OutcomeUnknown);
    assert_eq!(remote_of(db, &id).await, (None, None));
    assert_eq!(
        only_attempt(db, &id).await,
        (
            Some("outcome_unknown".to_owned()),
            Some("Mastodon did not answer within 30 seconds".to_owned())
        )
    );
    // An ambiguous result never runs again automatically, per 00002 D4.
    assert_eq!(step(db, &*adapter, &id).await.unwrap(), OutcomeUnknown);
    assert_eq!(adapter.calls(), 1);
    instance.close().await;
}

#[tokio::test]
async fn a_provider_with_no_adapter_refuses_at_the_commit() {
    let instance = Instance::start().await;
    let (organization_id, _) = set_up(&instance).await;
    let db = &instance.api_db;

    let id = insert_publication(db, &organization_id, Scheduled, "-1 minute").await;
    assert_eq!(step(db, &NoAdapter, &id).await.unwrap(), Preparing);
    assert_eq!(step(db, &NoAdapter, &id).await.unwrap(), Failed);
    assert_eq!(state_of(db, &id).await, Failed);
    assert_eq!(
        only_attempt(db, &id).await,
        (
            Some("failed".to_owned()),
            Some("Pipewhere cannot publish to Mastodon yet.".to_owned())
        )
    );

    let id = insert_publication(db, &organization_id, Preparing, "-1 minute").await;
    sqlx::query(
        "UPDATE pipewhere.channel SET provider = 'threads'
         WHERE id = (SELECT channel_id FROM pipewhere.publication WHERE id = $1)",
    )
    .bind(&id)
    .execute(db)
    .await
    .unwrap();
    assert_eq!(step(db, &NoAdapter, &id).await.unwrap(), Failed);
    assert_eq!(
        only_attempt(db, &id).await,
        (
            Some("failed".to_owned()),
            Some("Pipewhere cannot publish to Threads yet.".to_owned())
        )
    );
    instance.close().await;
}

#[tokio::test]
async fn a_commit_past_the_deadline_fails_without_calling_the_provider() {
    let instance = Instance::start().await;
    let (organization_id, _) = set_up(&instance).await;
    let db = &instance.api_db;
    let adapter = FakeAdapter::answering(Answer::Published);

    let id = insert_publication(db, &organization_id, Preparing, "-11 minutes").await;
    assert_eq!(step(db, &*adapter, &id).await.unwrap(), Failed);
    assert_eq!(state_of(db, &id).await, Failed);
    assert_eq!(
        only_attempt(db, &id).await,
        (Some("failed".to_owned()), Some(MISSED_ITS_TIME.to_owned()))
    );
    assert_eq!(adapter.calls(), 0);

    // Within its deadline, the same publication would have committed.
    let id = insert_publication(db, &organization_id, Preparing, "-9 minutes").await;
    assert_eq!(step(db, &*adapter, &id).await.unwrap(), Published);
    assert_eq!(adapter.calls(), 1);
    instance.close().await;
}

// The deadline applies to the start of the commit, per 00003 D2.
#[tokio::test]
async fn a_commit_that_started_in_time_is_not_cut_off() {
    let instance = Instance::start().await;
    let (organization_id, _) = set_up(&instance).await;
    let db = &instance.api_db;
    let adapter = FakeAdapter::holding(Answer::Published);

    let id = insert_publication(db, &organization_id, Preparing, "-1 minute").await;
    let running = run_step(db, &adapter, &id);
    adapter.wait_until_called().await;
    sqlx::query(
        "UPDATE pipewhere.publication SET planned_at = now() - interval '1 hour' WHERE id = $1",
    )
    .bind(&id)
    .execute(db)
    .await
    .unwrap();
    adapter.release();
    assert_eq!(running.await.unwrap(), Published);
    assert_eq!(state_of(db, &id).await, Published);
    instance.close().await;
}

#[tokio::test]
async fn a_cancelled_run_ends_its_attempt_as_cancelled() {
    let instance = Instance::start().await;
    let (organization_id, token) = set_up(&instance).await;
    let db = &instance.api_db;
    let adapter = FakeAdapter::answering(Answer::Published);

    let id = insert_publication(db, &organization_id, Scheduled, "-1 minute").await;
    assert_eq!(step(db, &*adapter, &id).await.unwrap(), Preparing);
    let cancel = format!("{}/cancel", publication_path(&organization_id, &id));
    let cancelled = ok_json(instance.post(&cancel, Some(&token), json!({})).await).await;
    assert_eq!(cancelled["state"], "cancelled");
    // API does not end the attempt, per 00003 D7.
    assert_eq!(only_attempt(db, &id).await, (None, None));

    assert_eq!(step(db, &*adapter, &id).await.unwrap(), Cancelled);
    assert_eq!(state_of(db, &id).await, Cancelled);
    assert_eq!(
        only_attempt(db, &id).await,
        (Some("cancelled".to_owned()), None)
    );
    assert_eq!(adapter.calls(), 0);

    // A client reads the new result.
    let path = format!("{}/attempts", publication_path(&organization_id, &id));
    let attempts = ok_json(instance.get(&path, Some(&token)).await).await;
    assert_eq!(attempts["attempts"][0]["result"], "cancelled");
    assert_eq!(attempts["attempts"][0]["reason"], Value::Null);

    // Another invocation leaves the ended attempt as it was.
    let ended = attempts_of(db, &id).await;
    tokio::time::sleep(Duration::from_millis(5)).await;
    assert_eq!(step(db, &*adapter, &id).await.unwrap(), Cancelled);
    assert_eq!(attempts_of(db, &id).await, ended);

    // Cancelled before any claim, it never had an attempt, so it gets none.
    let id = insert_publication(db, &organization_id, Cancelled, "-1 minute").await;
    assert_eq!(step(db, &*adapter, &id).await.unwrap(), Cancelled);
    assert_eq!(attempts_of(db, &id).await, []);
    assert_eq!(adapter.calls(), 0);
    instance.close().await;
}

// The interruption D8 names: a crash, an upgrade, or lost Restate state after
// the provider may have accepted, and before its answer was recorded.
#[tokio::test]
async fn a_step_that_finds_its_own_claim_records_outcome_unknown_and_never_calls_the_adapter() {
    let instance = Instance::start().await;
    let (organization_id, _) = set_up(&instance).await;
    let db = &instance.api_db;
    let adapter = FakeAdapter::holding(Answer::Published);

    let id = insert_publication(db, &organization_id, Preparing, "-1 minute").await;
    let interrupted = run_step(db, &adapter, &id);
    adapter.wait_until_called().await;
    interrupted.abort();
    assert!(interrupted.await.unwrap_err().is_cancelled());
    assert_eq!(state_of(db, &id).await, Publishing);
    assert_eq!(only_attempt(db, &id).await, (None, None));

    assert_eq!(step(db, &*adapter, &id).await.unwrap(), OutcomeUnknown);
    assert_eq!(state_of(db, &id).await, OutcomeUnknown);
    assert_eq!(remote_of(db, &id).await, (None, None));
    let (result, reason) = only_attempt(db, &id).await;
    assert_eq!(result.as_deref(), Some("outcome_unknown"));
    assert!(reason.unwrap().contains("may have started"));
    assert_eq!(adapter.calls(), 1, "the adapter is called once");

    assert_eq!(step(db, &*adapter, &id).await.unwrap(), OutcomeUnknown);
    assert_eq!(adapter.calls(), 1);
    instance.close().await;
}

// Restate can start another attempt while an earlier one is still running. The
// first to end the run wins, and the provider is still called only once.
#[tokio::test]
async fn a_late_answer_does_not_overwrite_outcome_unknown() {
    let instance = Instance::start().await;
    let (organization_id, _) = set_up(&instance).await;
    let db = &instance.api_db;
    let adapter = FakeAdapter::holding(Answer::Published);

    let id = insert_publication(db, &organization_id, Preparing, "-1 minute").await;
    let first = run_step(db, &adapter, &id);
    adapter.wait_until_called().await;
    assert_eq!(step(db, &*adapter, &id).await.unwrap(), OutcomeUnknown);
    let ended = attempts_of(db, &id).await;

    adapter.release();
    assert_eq!(first.await.unwrap(), Publishing, "the run stops");
    assert_eq!(state_of(db, &id).await, OutcomeUnknown);
    assert_eq!(remote_of(db, &id).await, (None, None));
    assert_eq!(attempts_of(db, &id).await, ended);
    assert_eq!(adapter.calls(), 1);
    instance.close().await;
}

// An answer belongs to the execution that asked. By the time it arrives, a person
// can have resolved the publication and retried it, so that `publishing` is a
// later run's.
#[tokio::test]
async fn a_late_answer_does_not_end_a_later_runs_attempt() {
    let instance = Instance::start().await;
    let (organization_id, token) = set_up(&instance).await;
    let db = &instance.api_db;
    let old = FakeAdapter::holding(Answer::Refused);
    let new = FakeAdapter::holding(Answer::Published);

    let id = insert_publication(db, &organization_id, Preparing, "-1 minute").await;
    let first = run_step(db, &old, &id);
    old.wait_until_called().await;
    assert_eq!(step(db, &*old, &id).await.unwrap(), OutcomeUnknown);

    let path = publication_path(&organization_id, &id);
    let resolve = instance
        .post(
            &format!("{path}/resolve"),
            Some(&token),
            json!({ "outcome": "failed" }),
        )
        .await;
    assert_eq!(ok_json(resolve).await["state"], "failed");
    let retry = instance
        .request(Method::POST, &format!("{path}/retry"), Some(&token))
        .header("idempotency-key", uuid::Uuid::now_v7().simple().to_string())
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(ok_json(retry).await["state"], "scheduled");
    assert_eq!(step(db, &*new, &id).await.unwrap(), Preparing);
    let second = run_step(db, &new, &id);
    new.wait_until_called().await;

    old.release();
    assert_eq!(
        first.await.unwrap(),
        Publishing,
        "the late answer changes nothing"
    );
    assert_eq!(state_of(db, &id).await, Publishing);
    assert_eq!(attempts_of(db, &id).await[1].result, None);

    new.release();
    assert_eq!(second.await.unwrap(), Published);
    assert_eq!(remote_of(db, &id).await.0.as_deref(), Some("109"));
    let attempts = attempts_of(db, &id).await;
    assert_eq!(attempts.len(), 2, "{attempts:?}");
    assert_eq!(attempts[0].result.as_deref(), Some("outcome_unknown"));
    assert_eq!(attempts[1].result.as_deref(), Some("published"));
    assert_eq!(attempts[1].reason, None);
    instance.close().await;
}

// A held lock's owner can roll back. PostgreSQL then does not check a waiting
// update's conditions again, so the deadline must be read once the lock is held.
#[tokio::test]
async fn a_commit_claim_that_waits_past_the_deadline_fails() {
    let instance = Instance::start().await;
    let (organization_id, _) = set_up(&instance).await;
    let db = &instance.api_db;
    let adapter = FakeAdapter::answering(Answer::Published);

    let id = insert_publication(db, &organization_id, Preparing, "-597 seconds").await;
    let mut held = db.begin().await.unwrap();
    sqlx::query("UPDATE pipewhere.publication SET state = 'cancelled' WHERE id = $1")
        .bind(&id)
        .execute(&mut *held)
        .await
        .unwrap();
    let commit = run_step(db, &adapter, &id);
    wait_for_blocked(&instance, 1).await;
    loop {
        let passed: bool = sqlx::query_scalar(
            "SELECT clock_timestamp() > planned_at + tolerance_seconds * interval '1 second'
             FROM pipewhere.publication WHERE id = $1",
        )
        .bind(&id)
        .fetch_one(&instance.admin)
        .await
        .unwrap();
        if passed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    held.rollback().await.unwrap();

    assert_eq!(commit.await.unwrap(), Failed);
    assert_eq!(state_of(db, &id).await, Failed);
    assert_eq!(
        only_attempt(db, &id).await,
        (Some("failed".to_owned()), Some(MISSED_ITS_TIME.to_owned()))
    );
    assert_eq!(adapter.calls(), 0);
    instance.close().await;
}

#[tokio::test]
async fn a_claim_moves_the_publication_and_starts_its_attempt_in_one_transaction() {
    let instance = Instance::start().await;
    let (organization_id, _) = set_up(&instance).await;
    let db = &instance.api_db;
    let adapter = FakeAdapter::answering(Answer::Published);

    let id = insert_publication(db, &organization_id, Scheduled, "-1 minute").await;
    // The claim's insert waits here, after its update has run.
    let mut held = instance.admin.begin().await.unwrap();
    sqlx::query("LOCK TABLE pipewhere.publication_attempt IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *held)
        .await
        .unwrap();
    let claim = run_step(db, &adapter, &id);
    wait_for_blocked(&instance, 1).await;
    assert_eq!(state_of(db, &id).await, Scheduled);
    held.commit().await.unwrap();

    assert_eq!(claim.await.unwrap(), Preparing);
    assert_eq!(state_of(db, &id).await, Preparing);
    assert_eq!(only_attempt(db, &id).await, (None, None));
    instance.close().await;
}

#[tokio::test]
async fn ending_a_run_leaves_another_publications_attempt_running() {
    let instance = Instance::start().await;
    let (organization_id, _) = set_up(&instance).await;
    let db = &instance.api_db;
    let adapter = FakeAdapter::answering(Answer::Published);

    let other = insert_publication(db, &organization_id, Preparing, "-1 minute").await;
    let published = insert_publication(db, &organization_id, Preparing, "-1 minute").await;
    assert_eq!(step(db, &*adapter, &published).await.unwrap(), Published);
    assert_eq!(only_attempt(db, &other).await, (None, None));

    let cancelled = insert_publication(db, &organization_id, Preparing, "-1 minute").await;
    sqlx::query("UPDATE pipewhere.publication SET state = 'cancelled' WHERE id = $1")
        .bind(&cancelled)
        .execute(db)
        .await
        .unwrap();
    assert_eq!(step(db, &*adapter, &cancelled).await.unwrap(), Cancelled);
    assert_eq!(only_attempt(db, &other).await, (None, None));
    instance.close().await;
}

// 00002 D19 requires an ended attempt for every move into `failed` or
// `outcome_unknown`.
#[tokio::test]
async fn a_move_with_no_running_attempt_to_end_is_an_error_and_changes_nothing() {
    let instance = Instance::start().await;
    let (organization_id, _) = set_up(&instance).await;
    let db = &instance.api_db;
    let adapter = FakeAdapter::answering(Answer::Published);

    // Another publication's running attempt does not stand in for its own.
    let other = insert_publication(db, &organization_id, Preparing, "-1 minute").await;
    let unclaimed = insert_publication(db, &organization_id, Preparing, "-1 hour").await;
    let in_time = insert_publication(db, &organization_id, Preparing, "-1 minute").await;
    let interrupted = insert_publication(db, &organization_id, Publishing, "-1 minute").await;
    for id in [&unclaimed, &in_time, &interrupted] {
        sqlx::query("DELETE FROM pipewhere.publication_attempt WHERE publication_id = $1")
            .bind(id)
            .execute(db)
            .await
            .unwrap();
    }
    assert!(matches!(
        step(db, &*adapter, &unclaimed).await,
        Err(sqlx::Error::RowNotFound)
    ));
    assert_eq!(state_of(db, &unclaimed).await, Preparing);
    assert!(matches!(
        step(db, &*adapter, &in_time).await,
        Err(sqlx::Error::RowNotFound)
    ));
    assert_eq!(state_of(db, &in_time).await, Preparing);
    assert_eq!(adapter.calls(), 0, "nothing would record what a post did");
    assert!(matches!(
        step(db, &*adapter, &interrupted).await,
        Err(sqlx::Error::RowNotFound)
    ));
    assert_eq!(state_of(db, &interrupted).await, Publishing);
    assert_eq!(only_attempt(db, &other).await, (None, None));
    instance.close().await;
}

#[tokio::test]
async fn two_claims_racing_make_one_attempt_and_the_loser_stops() {
    let instance = Instance::start().await;
    let (organization_id, _) = set_up(&instance).await;
    let db = &instance.api_db;
    let adapter = FakeAdapter::answering(Answer::Published);

    let id = insert_publication(db, &organization_id, Scheduled, "-1 minute").await;
    // Both read `scheduled`, and wait for this transaction at their updates.
    let mut held = db.begin().await.unwrap();
    sqlx::query("UPDATE pipewhere.publication SET body = body WHERE id = $1")
        .bind(&id)
        .execute(&mut *held)
        .await
        .unwrap();
    let first = run_step(db, &adapter, &id);
    let second = run_step(db, &adapter, &id);
    wait_for_blocked(&instance, 2).await;
    held.commit().await.unwrap();

    let mut answers = [first.await.unwrap(), second.await.unwrap()];
    answers.sort_by_key(|state| *state != Preparing);
    assert_eq!(answers, [Preparing, Scheduled]);
    assert_eq!(state_of(db, &id).await, Preparing);
    assert_eq!(only_attempt(db, &id).await, (None, None));
    assert_eq!(adapter.calls(), 0);
    instance.close().await;
}

#[tokio::test]
async fn a_cancel_or_an_edit_racing_a_claim_decides_it_by_who_commits_first() {
    let instance = Instance::start().await;
    let (organization_id, _) = set_up(&instance).await;
    let db = &instance.api_db;
    let adapter = FakeAdapter::answering(Answer::Published);

    let id = insert_publication(db, &organization_id, Scheduled, "-1 minute").await;
    let mut held = db.begin().await.unwrap();
    sqlx::query("UPDATE pipewhere.publication SET state = 'cancelled' WHERE id = $1")
        .bind(&id)
        .execute(&mut *held)
        .await
        .unwrap();
    let claim = run_step(db, &adapter, &id);
    wait_for_blocked(&instance, 1).await;
    held.commit().await.unwrap();
    assert_eq!(claim.await.unwrap(), Scheduled, "the claim lost");
    assert_eq!(state_of(db, &id).await, Cancelled);
    assert_eq!(attempts_of(db, &id).await, []);

    let id = insert_publication(db, &organization_id, Scheduled, "-1 minute").await;
    let mut held = db.begin().await.unwrap();
    sqlx::query(
        "UPDATE pipewhere.publication SET planned_at = now() + interval '1 hour' WHERE id = $1",
    )
    .bind(&id)
    .execute(&mut *held)
    .await
    .unwrap();
    let claim = run_step(db, &adapter, &id);
    wait_for_blocked(&instance, 1).await;
    held.commit().await.unwrap();
    assert_eq!(claim.await.unwrap(), Scheduled, "the time moved later");
    assert_eq!(state_of(db, &id).await, Scheduled);
    assert_eq!(attempts_of(db, &id).await, []);

    let id = insert_publication(db, &organization_id, Scheduled, "-1 minute").await;
    let mut held = db.begin().await.unwrap();
    sqlx::query("UPDATE pipewhere.publication SET state = 'cancelled' WHERE id = $1")
        .bind(&id)
        .execute(&mut *held)
        .await
        .unwrap();
    let claim = run_step(db, &adapter, &id);
    wait_for_blocked(&instance, 1).await;
    held.rollback().await.unwrap();
    assert_eq!(claim.await.unwrap(), Preparing, "the cancel rolled back");
    assert_eq!(only_attempt(db, &id).await, (None, None));
    instance.close().await;
}

#[tokio::test]
async fn a_cancel_committed_first_stops_the_commit_claim() {
    let instance = Instance::start().await;
    let (organization_id, _) = set_up(&instance).await;
    let db = &instance.api_db;
    let adapter = FakeAdapter::answering(Answer::Published);

    let id = insert_publication(db, &organization_id, Preparing, "-1 minute").await;
    let mut held = db.begin().await.unwrap();
    sqlx::query("UPDATE pipewhere.publication SET state = 'cancelled' WHERE id = $1")
        .bind(&id)
        .execute(&mut *held)
        .await
        .unwrap();
    let commit = run_step(db, &adapter, &id);
    wait_for_blocked(&instance, 1).await;
    held.commit().await.unwrap();
    assert_eq!(commit.await.unwrap(), Cancelled);
    assert_eq!(
        only_attempt(db, &id).await,
        (Some("cancelled".to_owned()), None)
    );
    assert_eq!(adapter.calls(), 0);

    // A cancel that rolls back leaves the commit to go on.
    let id = insert_publication(db, &organization_id, Preparing, "-1 minute").await;
    let mut held = db.begin().await.unwrap();
    sqlx::query("UPDATE pipewhere.publication SET state = 'cancelled' WHERE id = $1")
        .bind(&id)
        .execute(&mut *held)
        .await
        .unwrap();
    let commit = run_step(db, &adapter, &id);
    wait_for_blocked(&instance, 1).await;
    held.rollback().await.unwrap();
    assert_eq!(commit.await.unwrap(), Published);
    assert_eq!(adapter.calls(), 1);
    instance.close().await;
}

// The state a step read can change before its update reaches the row. The update
// then changes nothing, and the step answers what the publication holds now.
#[tokio::test]
async fn a_step_that_loses_its_move_answers_what_the_publication_holds_now() {
    let instance = Instance::start().await;
    let (organization_id, _) = set_up(&instance).await;
    let db = &instance.api_db;
    let adapter = FakeAdapter::answering(Answer::Published);

    let id = insert_publication(db, &organization_id, Publishing, "-1 minute").await;
    let mut held = db.begin().await.unwrap();
    sqlx::query(
        "UPDATE pipewhere.publication SET state = 'published', remote_id = '109' WHERE id = $1",
    )
    .bind(&id)
    .execute(&mut *held)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE pipewhere.publication_attempt SET result = 'published', ended_at = now()
         WHERE publication_id = $1",
    )
    .bind(&id)
    .execute(&mut *held)
    .await
    .unwrap();
    let running = run_step(db, &adapter, &id);
    wait_for_blocked(&instance, 1).await;
    held.commit().await.unwrap();

    assert_eq!(running.await.unwrap(), Published);
    assert_eq!(state_of(db, &id).await, Published);
    assert_eq!(remote_of(db, &id).await.0.as_deref(), Some("109"));
    assert_eq!(
        only_attempt(db, &id).await,
        (Some("published".to_owned()), None)
    );
    assert_eq!(adapter.calls(), 0);
    instance.close().await;
}

// A commit claim waits for the row before it reads the deadline, so a cancel
// holding the row decides by its commit or rollback.
#[tokio::test]
async fn a_cancel_racing_the_missed_deadline_decides_it_by_who_commits_first() {
    let instance = Instance::start().await;
    let (organization_id, _) = set_up(&instance).await;
    let db = &instance.api_db;
    let adapter = FakeAdapter::answering(Answer::Published);

    for (commit, state, result, reason) in [
        (true, Cancelled, "cancelled", None),
        (false, Failed, "failed", Some(MISSED_ITS_TIME.to_owned())),
    ] {
        let id = insert_publication(db, &organization_id, Preparing, "-1 hour").await;
        let mut held = db.begin().await.unwrap();
        sqlx::query("UPDATE pipewhere.publication SET state = 'cancelled' WHERE id = $1")
            .bind(&id)
            .execute(&mut *held)
            .await
            .unwrap();
        let running = run_step(db, &adapter, &id);
        wait_for_blocked(&instance, 1).await;
        if commit {
            held.commit().await.unwrap();
        } else {
            held.rollback().await.unwrap();
        }
        assert_eq!(running.await.unwrap(), state);
        assert_eq!(state_of(db, &id).await, state);
        assert_eq!(
            only_attempt(db, &id).await,
            (Some(result.to_owned()), reason)
        );
    }
    assert_eq!(adapter.calls(), 0);
    instance.close().await;
}

// A missed deadline fails while the commit claim holds the row. Failing it after
// the lock is released would let this cancel win, and could fail a later run.
#[tokio::test]
async fn a_cancel_queued_behind_a_missed_deadline_finds_it_failed() {
    let instance = Instance::start().await;
    let (organization_id, token) = set_up(&instance).await;
    let db = &instance.api_db;
    let adapter = FakeAdapter::answering(Answer::Published);

    let id = insert_publication(db, &organization_id, Preparing, "-1 hour").await;
    let mut held = db.begin().await.unwrap();
    sqlx::query("UPDATE pipewhere.publication SET body = body WHERE id = $1")
        .bind(&id)
        .execute(&mut *held)
        .await
        .unwrap();
    let running = run_step(db, &adapter, &id);
    wait_for_blocked(&instance, 1).await;

    let path = format!("{}/cancel", publication_path(&organization_id, &id));
    let (cancel, ()) = tokio::join!(instance.post(&path, Some(&token), json!({})), async {
        wait_for_blocked(&instance, 2).await;
        held.rollback().await.unwrap();
    });

    assert_eq!(
        refusal(cancel).await,
        (StatusCode::CONFLICT, "state_conflict".to_owned())
    );
    assert_eq!(running.await.unwrap(), Failed);
    assert_eq!(state_of(db, &id).await, Failed);
    assert_eq!(
        only_attempt(db, &id).await,
        (Some("failed".to_owned()), Some(MISSED_ITS_TIME.to_owned()))
    );
    assert_eq!(adapter.calls(), 0);
    instance.close().await;
}

// 00003 D8's reading of a claim that changed no row, when another claim held it.
#[tokio::test]
async fn a_commit_claim_that_finds_publishing_records_outcome_unknown() {
    let instance = Instance::start().await;
    let (organization_id, _) = set_up(&instance).await;
    let db = &instance.api_db;
    let adapter = FakeAdapter::answering(Answer::Published);

    let id = insert_publication(db, &organization_id, Preparing, "-1 minute").await;
    let mut held = db.begin().await.unwrap();
    sqlx::query("UPDATE pipewhere.publication SET state = 'publishing' WHERE id = $1")
        .bind(&id)
        .execute(&mut *held)
        .await
        .unwrap();
    let commit = run_step(db, &adapter, &id);
    wait_for_blocked(&instance, 1).await;
    held.commit().await.unwrap();
    assert_eq!(commit.await.unwrap(), OutcomeUnknown);
    assert_eq!(state_of(db, &id).await, OutcomeUnknown);
    assert_eq!(adapter.calls(), 0);
    instance.close().await;
}

// The claim commits before the provider is called, so the publication is
// visibly `publishing` during the call, and the API refuses to cancel or edit it.
#[tokio::test]
async fn a_cancel_after_the_commit_claim_answers_409() {
    let instance = Instance::start().await;
    let (organization_id, token) = set_up(&instance).await;
    let db = &instance.api_db;
    let adapter = FakeAdapter::holding(Answer::Published);

    let id = insert_publication(db, &organization_id, Preparing, "-1 minute").await;
    let commit = run_step(db, &adapter, &id);
    adapter.wait_until_called().await;
    assert_eq!(state_of(db, &id).await, Publishing);

    let conflict = (StatusCode::CONFLICT, "state_conflict".to_owned());
    let path = publication_path(&organization_id, &id);
    let cancel = instance
        .post(&format!("{path}/cancel"), Some(&token), json!({}))
        .await;
    assert_eq!(refusal(cancel).await, conflict);
    let edit = instance
        .patch(&path, Some(&token), json!({ "body": "Changed" }))
        .await;
    assert_eq!(refusal(edit).await, conflict);

    adapter.release();
    assert_eq!(commit.await.unwrap(), Published);
    assert_eq!(
        *adapter.seen.lock().unwrap(),
        [(Provider::Mastodon, "Copy".to_owned())]
    );
    instance.close().await;
}

// The held tests above force each order. This races real requests, and checks
// that the two answers always agree.
#[tokio::test]
async fn a_cancel_and_a_commit_claim_always_agree_on_who_won() {
    let instance = Instance::start().await;
    let (organization_id, token) = set_up(&instance).await;
    let db = &instance.api_db;

    for _ in 0..25 {
        let adapter = FakeAdapter::answering(Answer::Published);
        let id = insert_publication(db, &organization_id, Preparing, "-1 minute").await;
        let path = format!("{}/cancel", publication_path(&organization_id, &id));
        let (commit, cancel) = tokio::join!(
            run_step(db, &adapter, &id),
            instance.post(&path, Some(&token), json!({})),
        );
        let (result, reason) = only_attempt(db, &id).await;
        assert_eq!(reason, None);
        if cancel.status().is_success() {
            assert_eq!(commit.unwrap(), Cancelled);
            assert_eq!(state_of(db, &id).await, Cancelled);
            assert_eq!(result.as_deref(), Some("cancelled"));
            assert_eq!(adapter.calls(), 0);
        } else {
            assert_eq!(cancel.status(), StatusCode::CONFLICT);
            assert_eq!(commit.unwrap(), Published);
            assert_eq!(state_of(db, &id).await, Published);
            assert_eq!(result.as_deref(), Some("published"));
            assert_eq!(adapter.calls(), 1);
        }
    }
    instance.close().await;
}
