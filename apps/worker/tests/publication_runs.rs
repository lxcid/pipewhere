//! Runs publications through a real Restate, the image Compose pins, against a
//! real API and PostgreSQL. Each test starts its own Restate with Docker, and
//! the Restate container reaches Worker through the Docker host.

#[path = "../../api/tests/support/mod.rs"]
mod support;

use std::future::Future;
use std::net::TcpListener;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use pipewhere_api::ids::new_id;
use pipewhere_api::runs::{Adapter, Commit, CommitResult, NoAdapter};
use pipewhere_worker::poller::Poller;
use pipewhere_worker::publication::Publication;
use pipewhere_worker::register;
use reqwest::{Method, StatusCode};
use restate_sdk::endpoint::Endpoint;
use restate_sdk::http_server::HttpServer;
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use support::{Instance, ok_json};
use tokio::runtime::Runtime;

const MISSED_ITS_TIME: &str =
    "Missed its time: its provider commit had not started by its deadline.";

fn restate_image() -> String {
    let compose = include_str!("../../../compose.yaml");
    let image = compose
        .split_whitespace()
        .find(|word| word.starts_with("restatedev/restate:"))
        .expect("compose.yaml pins Restate's image");
    image.to_owned()
}

struct Restate {
    container: String,
    admin_url: String,
    ingress_url: String,
}

impl Restate {
    async fn start() -> Self {
        let output = Command::new("docker")
            .args(["run", "--rm", "-d"])
            .args(["-p", "127.0.0.1::8080", "-p", "127.0.0.1::9070"])
            .arg("--add-host=host.docker.internal:host-gateway")
            .arg(restate_image())
            .output()
            .expect("docker runs");
        assert!(output.status.success(), "{output:?}");
        let container = String::from_utf8(output.stdout).unwrap().trim().to_owned();
        let restate = Self {
            admin_url: format!("http://{}", published_address(&container, 9070)),
            ingress_url: format!("http://{}", published_address(&container, 8080)),
            container,
        };
        let health = format!("{}/health", restate.admin_url);
        wait_until("Restate is healthy", async || {
            reqwest::get(&health)
                .await
                .is_ok_and(|response| response.status().is_success())
        })
        .await;
        restate
    }

    async fn query(&self, sql: &str) -> Vec<Value> {
        let response = reqwest::Client::new()
            .post(format!("{}/query", self.admin_url))
            .header("accept", "application/json")
            .json(&json!({ "query": sql }))
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body: Value = response.json().await.unwrap();
        assert!(status.is_success(), "{sql}: {status} {body}");
        body["rows"].as_array().unwrap().clone()
    }

    async fn wait_until_idle(&self) {
        wait_until("Restate has no invocation left to run", async || {
            self.query("SELECT id FROM sys_invocation WHERE status != 'completed'")
                .await
                .is_empty()
        })
        .await;
    }
}

impl Drop for Restate {
    fn drop(&mut self) {
        Command::new("docker")
            .args(["rm", "-f", &self.container])
            .output()
            .ok();
    }
}

fn published_address(container: &str, port: u16) -> String {
    let output = Command::new("docker")
        .args(["port", container, &port.to_string()])
        .output()
        .expect("docker runs");
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .next()
        .expect("the port is published")
        .to_owned()
}

/// A Worker on a runtime of its own, so that shutting the runtime down drops
/// everything it was doing, as a crash does. `serve_with_cancel` would wait for
/// the open invocation instead.
struct Worker {
    runtime: Option<Runtime>,
    port: u16,
}

impl Worker {
    async fn start<A: Adapter + Send + Sync + 'static>(
        db: &PgPool,
        port: u16,
        adapter: Arc<A>,
    ) -> Self {
        let options = (*db.connect_options()).clone();
        Self::serving(port, move || {
            let db = PgPoolOptions::new().connect_lazy_with(options);
            Endpoint::builder()
                .bind(Publication { db, adapter })
                .build()
        })
        .await
    }

    async fn serving(port: u16, endpoint: impl FnOnce() -> Endpoint + Send + 'static) -> Self {
        let listener = bind(port).await;
        let port = listener.local_addr().unwrap().port();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        runtime.spawn(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            HttpServer::new(endpoint()).serve(listener).await;
        });
        Self {
            runtime: Some(runtime),
            port,
        }
    }

    fn url(&self) -> String {
        format!("http://host.docker.internal:{}", self.port)
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.runtime.take().unwrap().shutdown_background();
    }
}

// The port of a Worker just stopped stays taken until its tasks have dropped.
async fn bind(port: u16) -> TcpListener {
    let started = Instant::now();
    loop {
        match TcpListener::bind(("0.0.0.0", port)) {
            Ok(listener) => {
                listener.set_nonblocking(true).unwrap();
                return listener;
            }
            Err(error) if started.elapsed() > Duration::from_secs(10) => {
                panic!("port {port} stayed taken: {error}")
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
}

async fn wait_until(what: &str, mut condition: impl AsyncFnMut() -> bool) {
    let started = Instant::now();
    while !condition().await {
        assert!(
            started.elapsed() < Duration::from_secs(45),
            "timed out waiting until {what}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

enum Answer {
    Published,
    /// Holds the call open until the runtime it runs on stops.
    Hold,
}

struct FakeAdapter {
    answer: Answer,
    calls: AtomicUsize,
}

impl FakeAdapter {
    fn answering(answer: Answer) -> Arc<Self> {
        Arc::new(Self {
            answer,
            calls: AtomicUsize::new(0),
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Adapter for FakeAdapter {
    fn commit(&self, commit: &Commit) -> impl Future<Output = CommitResult> + Send {
        assert_eq!(commit.body, "Copy");
        self.calls.fetch_add(1, Ordering::SeqCst);
        async move {
            match self.answer {
                Answer::Published => CommitResult::Published {
                    remote_id: "109".to_owned(),
                    remote_url: Some("https://mastodon.example/@a/109".to_owned()),
                },
                Answer::Hold => std::future::pending().await,
            }
        }
    }
}

/// One organization on a real API, with one Mastodon channel.
struct Organization {
    instance: Instance,
    id: String,
    token: String,
    channel_id: String,
}

impl Organization {
    async fn start() -> Self {
        let instance = Instance::start().await;
        let owner = instance.sign_up("owner@example.com").await;
        let id = instance.create_organization(&owner, "acme").await;
        let token = instance.session_token(&owner).await;
        let channel_id = new_id("channel");
        sqlx::query(
            "INSERT INTO pipewhere.channel (id, organization_id, provider)
             VALUES ($1, $2, 'mastodon')",
        )
        .bind(&channel_id)
        .bind(&id)
        .execute(&instance.api_db)
        .await
        .unwrap();
        Self {
            instance,
            id,
            token,
            channel_id,
        }
    }

    /// `planned` adds fields to the request, which otherwise publishes now.
    async fn create_publication(&self, planned: Value) -> String {
        let mut body = json!({ "channel_id": self.channel_id, "body": "Copy" });
        body.as_object_mut()
            .unwrap()
            .extend(planned.as_object().unwrap().clone());
        let path = format!("/v1/orgs/{}/publications", self.id);
        let response = self
            .instance
            .request(Method::POST, &path, Some(&self.token))
            .header("idempotency-key", new_id("publication"))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        response.json::<Value>().await.unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    /// A publication written directly, for a state or an id API would not give.
    /// `planned` is an interval from now.
    async fn insert_publication(&self, id: &str, state: &str, planned: &str) {
        let db = &self.instance.api_db;
        let content_id = new_id("content");
        sqlx::query(
            "INSERT INTO pipewhere.content (id, organization_id, body) VALUES ($1, $2, 'Source')",
        )
        .bind(&content_id)
        .bind(&self.id)
        .execute(db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pipewhere.publication
                 (id, organization_id, channel_id, content_id, body, state, planned_at,
                  tolerance_seconds, remote_id)
             VALUES ($1, $2, $3, $4, 'Copy', $5, now() + $6::text::interval, 600,
                 CASE WHEN $5 = 'published' THEN 'earlier' END)",
        )
        .bind(id)
        .bind(&self.id)
        .bind(&self.channel_id)
        .bind(&content_id)
        .bind(state)
        .bind(planned)
        .execute(db)
        .await
        .unwrap();
    }

    async fn read(&self, publication_id: &str) -> Value {
        ok_json(
            self.instance
                .get(
                    &format!("/v1/orgs/{}/publications/{publication_id}", self.id),
                    Some(&self.token),
                )
                .await,
        )
        .await
    }

    async fn attempts(&self, publication_id: &str) -> Vec<Value> {
        let path = format!(
            "/v1/orgs/{}/publications/{publication_id}/attempts",
            self.id
        );
        let list = ok_json(self.instance.get(&path, Some(&self.token)).await).await;
        list["attempts"].as_array().unwrap().clone()
    }

    async fn wait_for_state(&self, publication_id: &str, state: &str) {
        wait_until(&format!("{publication_id} is {state}"), async || {
            self.read(publication_id).await["state"] == state
        })
        .await;
    }
}

#[tokio::test]
async fn the_poller_runs_due_work_and_leaves_the_rest_waiting() {
    let org = Organization::start().await;
    let restate = Restate::start().await;
    let db = org.instance.api_db.clone();
    let worker = Worker::start(&db, 0, Arc::new(NoAdapter)).await;
    register(&restate.admin_url, &worker.url()).await.unwrap();

    let due = org.create_publication(json!({})).await;
    let late = org
        .create_publication(json!({ "planned_at": "2020-01-01T00:00:00Z" }))
        .await;
    let waiting = org
        .create_publication(json!({ "planned_at": "9000-01-01T00:00:00Z" }))
        .await;

    let poller = Poller::new(db, restate.ingress_url.clone()).unwrap();
    let polling = tokio::spawn(async move { poller.run().await });

    org.wait_for_state(&due, "failed").await;
    let attempts = org.attempts(&due).await;
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0]["result"], "failed");
    assert_eq!(
        attempts[0]["reason"],
        "Pipewhere cannot publish to Mastodon yet."
    );

    org.wait_for_state(&late, "failed").await;
    let attempts = org.attempts(&late).await;
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0]["reason"], MISSED_ITS_TIME);

    restate.wait_until_idle().await;
    assert_eq!(org.read(&waiting).await["state"], "scheduled");
    assert!(org.attempts(&waiting).await.is_empty());

    polling.abort();
    drop(worker);
    drop(restate);
    org.instance.close().await;
}

#[tokio::test]
async fn sending_a_run_again_publishes_once() {
    let org = Organization::start().await;
    let restate = Restate::start().await;
    let db = org.instance.api_db.clone();
    let adapter = FakeAdapter::answering(Answer::Published);
    let worker = Worker::start(&db, 0, adapter.clone()).await;
    register(&restate.admin_url, &worker.url()).await.unwrap();
    let poller = Poller::new(db, restate.ingress_url.clone()).unwrap();

    let due = org.create_publication(json!({})).await;
    for _ in 0..5 {
        poller.send_run(&due).await.unwrap();
    }
    org.wait_for_state(&due, "published").await;
    restate.wait_until_idle().await;

    let publication = org.read(&due).await;
    assert_eq!(publication["remote_id"], "109");
    assert_eq!(publication["remote_url"], "https://mastodon.example/@a/109");
    assert_eq!(adapter.calls(), 1);
    let attempts = org.attempts(&due).await;
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0]["result"], "published");

    drop(worker);
    drop(restate);
    org.instance.close().await;
}

// Sent directly, because the poller would not send it: an edit can move the time
// later while a run waits in Restate's queue, per 00003 D16.
#[tokio::test]
async fn a_run_sent_before_the_planned_time_changes_nothing() {
    let org = Organization::start().await;
    let restate = Restate::start().await;
    let db = org.instance.api_db.clone();
    let adapter = FakeAdapter::answering(Answer::Published);
    let worker = Worker::start(&db, 0, adapter.clone()).await;
    register(&restate.admin_url, &worker.url()).await.unwrap();
    let poller = Poller::new(db, restate.ingress_url.clone()).unwrap();

    let waiting = org
        .create_publication(json!({ "planned_at": "9000-01-01T00:00:00Z" }))
        .await;
    poller.send_run(&waiting).await.unwrap();
    restate.wait_until_idle().await;

    assert_eq!(org.read(&waiting).await["state"], "scheduled");
    assert!(org.attempts(&waiting).await.is_empty());
    assert_eq!(adapter.calls(), 0);

    drop(worker);
    drop(restate);
    org.instance.close().await;
}

// A database error must leave the run to Restate's retries, not end it, because
// a step ended that way would strand its publication.
#[tokio::test]
async fn a_step_that_fails_runs_again() {
    let org = Organization::start().await;
    let restate = Restate::start().await;
    let db = org.instance.api_db.clone();
    let adapter = FakeAdapter::answering(Answer::Published);
    let worker = Worker::start(&db, 0, adapter.clone()).await;
    register(&restate.admin_url, &worker.url()).await.unwrap();
    let poller = Poller::new(db.clone(), restate.ingress_url.clone()).unwrap();

    // The first step finds no such publication, which is an error.
    let id = new_id("publication");
    poller.send_run(&id).await.unwrap();
    wait_until("the first step has failed", async || {
        !restate
            .query("SELECT id FROM sys_invocation WHERE last_failure IS NOT NULL")
            .await
            .is_empty()
    })
    .await;

    org.insert_publication(&id, "scheduled", "0 seconds").await;

    org.wait_for_state(&id, "published").await;
    assert_eq!(adapter.calls(), 1);

    drop(worker);
    drop(restate);
    org.instance.close().await;
}

#[tokio::test]
async fn a_tick_fails_until_restate_can_take_the_run() {
    let org = Organization::start().await;
    let restate = Restate::start().await;
    let db = org.instance.api_db.clone();

    let closed = PgPoolOptions::new().connect_lazy_with((*db.connect_options()).clone());
    closed.close().await;
    Poller::new(closed, restate.ingress_url.clone())
        .unwrap()
        .tick()
        .await
        .unwrap_err();

    // With nothing due, a tick sends nothing, so even a closed port is fine.
    let unreachable = Poller::new(db.clone(), "http://127.0.0.1:9".to_owned()).unwrap();
    unreachable.tick().await.unwrap();

    let due = org.create_publication(json!({})).await;
    unreachable.tick().await.unwrap_err();

    // Restate answers 404 until Worker has registered.
    let poller = Poller::new(db.clone(), restate.ingress_url.clone()).unwrap();
    poller.tick().await.unwrap_err();
    assert_eq!(org.read(&due).await["state"], "scheduled");

    let worker = Worker::start(&db, 0, Arc::new(NoAdapter)).await;
    register(&restate.admin_url, &worker.url()).await.unwrap();
    poller.tick().await.unwrap();
    org.wait_for_state(&due, "failed").await;

    drop(worker);
    drop(restate);
    org.instance.close().await;
}

#[tokio::test]
async fn a_tick_takes_the_oldest_due_work_first() {
    let org = Organization::start().await;
    let db = org.instance.api_db.clone();
    let poller = Poller::new(db.clone(), "http://127.0.0.1:9".to_owned()).unwrap();

    org.insert_publication(&new_id("publication"), "scheduled", "1 hour")
        .await;
    assert!(poller.due().await.unwrap().is_empty());

    // Newest first, so the order the rows were written in cannot pass for the
    // order of their times.
    let mut oldest_first = Vec::new();
    for minutes in 1..=101 {
        let id = new_id("publication");
        org.insert_publication(&id, "scheduled", &format!("-{minutes} minutes"))
            .await;
        oldest_first.insert(0, id);
    }
    for state in [
        "preparing",
        "publishing",
        "published",
        "failed",
        "outcome_unknown",
        "cancelled",
    ] {
        org.insert_publication(&new_id("publication"), state, "-1 year")
            .await;
    }

    assert_eq!(poller.due().await.unwrap(), oldest_first[..100]);

    // An index scan returns this order without being asked, so only a query
    // without the index shows that the query asks for it.
    sqlx::query("DROP INDEX pipewhere.publication_due")
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(poller.due().await.unwrap(), oldest_first[..100]);

    org.instance.close().await;
}

/// 00001 D14: Restate keeps a deployment's handlers when its URI registers again
/// without `force`. An installation upgrading to the first handler is this case.
#[tokio::test]
async fn registering_again_picks_up_a_new_handler() {
    let org = Organization::start().await;
    let restate = Restate::start().await;
    let db = org.instance.api_db.clone();
    let empty = Worker::serving(0, || Endpoint::builder().build()).await;
    register(&restate.admin_url, &empty.url()).await.unwrap();
    let port = empty.port;
    drop(empty);

    let worker = Worker::start(&db, port, Arc::new(NoAdapter)).await;
    register(&restate.admin_url, &worker.url()).await.unwrap();
    let id = org.create_publication(json!({})).await;
    Poller::new(db, restate.ingress_url.clone())
        .unwrap()
        .send_run(&id)
        .await
        .unwrap();
    org.wait_for_state(&id, "failed").await;

    drop(worker);
    drop(restate);
    org.instance.close().await;
}

/// 00001 D14: the code changes under a run held inside a step. The Worker that
/// took the run stops, and a Worker with another adapter registers under the
/// same URI, as `docker compose up --build` does.
#[tokio::test]
async fn a_commit_interrupted_by_an_upgrade_never_repeats() {
    let org = Organization::start().await;
    let restate = Restate::start().await;
    let db = org.instance.api_db.clone();
    let holding = FakeAdapter::answering(Answer::Hold);
    let old = Worker::start(&db, 0, holding.clone()).await;
    register(&restate.admin_url, &old.url()).await.unwrap();
    let poller = Poller::new(db.clone(), restate.ingress_url.clone()).unwrap();

    let id = org.create_publication(json!({})).await;
    poller.send_run(&id).await.unwrap();
    wait_until("the adapter is called", async || holding.calls() == 1).await;
    assert_eq!(org.read(&id).await["state"], "publishing");

    // 00003 D6: the journal holds `step` entries and nothing else. The second is
    // the commit, still running.
    let journal = restate
        .query(&format!(
            "SELECT j.entry_type, j.name FROM sys_journal j
             JOIN sys_invocation i ON j.id = i.id
             WHERE i.target_service_key = '{id}'
             ORDER BY j.index"
        ))
        .await;
    assert_eq!(
        journal,
        [
            json!({ "entry_type": "Command: Input", "name": "" }),
            json!({ "entry_type": "Command: Run", "name": "step" }),
            json!({ "entry_type": "Notification: Run" }),
            json!({ "entry_type": "Command: Run", "name": "step" }),
        ]
    );

    let port = old.port;
    drop(old);
    let publishing = FakeAdapter::answering(Answer::Published);
    let new = Worker::start(&db, port, publishing.clone()).await;
    register(&restate.admin_url, &new.url()).await.unwrap();

    org.wait_for_state(&id, "outcome_unknown").await;
    restate.wait_until_idle().await;
    assert_eq!(holding.calls(), 1);
    assert_eq!(publishing.calls(), 0);
    let attempts = org.attempts(&id).await;
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0]["result"], "outcome_unknown");

    drop(new);
    drop(restate);
    org.instance.close().await;
}

/// 00003 D6: the first step replays from its stored state, so it writes no second
/// attempt, and the next step runs live on the new code.
#[tokio::test]
async fn a_run_held_before_its_commit_continues_on_the_new_code() {
    let org = Organization::start().await;
    let restate = Restate::start().await;
    let db = org.instance.api_db.clone();
    let unused = FakeAdapter::answering(Answer::Hold);
    let old = Worker::start(&db, 0, unused.clone()).await;
    register(&restate.admin_url, &old.url()).await.unwrap();
    let poller = Poller::new(db.clone(), restate.ingress_url.clone()).unwrap();

    let id = new_id("publication");
    org.insert_publication(&id, "scheduled", "0 seconds").await;

    // The commit claim reads the channel, and the first step does not, so this
    // holds the run between the two.
    let mut lock = org.instance.admin.begin().await.unwrap();
    sqlx::query("LOCK TABLE pipewhere.channel IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *lock)
        .await
        .unwrap();

    poller.send_run(&id).await.unwrap();
    wait_until("the commit claim waits for the lock", async || {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity
             WHERE wait_event_type = 'Lock' AND query LIKE '%publishing%'",
        )
        .fetch_one(&org.instance.admin)
        .await
        .unwrap();
        waiting == 1
    })
    .await;
    let attempts_before: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pipewhere.publication_attempt WHERE publication_id = $1",
    )
    .bind(&id)
    .fetch_one(&org.instance.admin)
    .await
    .unwrap();
    assert_eq!(attempts_before, 1);

    let port = old.port;
    drop(old);
    let publishing = FakeAdapter::answering(Answer::Published);
    let new = Worker::start(&db, port, publishing.clone()).await;
    register(&restate.admin_url, &new.url()).await.unwrap();
    lock.rollback().await.unwrap();

    org.wait_for_state(&id, "published").await;
    assert_eq!(unused.calls(), 0);
    assert_eq!(publishing.calls(), 1);
    assert_eq!(org.attempts(&id).await.len(), 1);

    drop(new);
    drop(restate);
    org.instance.close().await;
}
