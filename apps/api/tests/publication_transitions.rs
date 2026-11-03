//! Editing, cancelling, retrying, and resolving publications, per 00002 D2–D6
//! and D13, and 00003 D11. Only the worker moves a publication into
//! `preparing`, `publishing`, or `outcome_unknown`, so these tests insert
//! publications in each state directly.

mod support;

use chrono::{DateTime, Utc};
use pipewhere_api::ids::new_id;
use reqwest::header::CONTENT_TYPE;
use reqwest::{Method, RequestBuilder, Response, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Instance, ok_json, refusal};

const STATES: [&str; 7] = [
    "scheduled",
    "preparing",
    "publishing",
    "published",
    "failed",
    "outcome_unknown",
    "cancelled",
];

const ACTIONS: [&str; 6] = [
    "edit",
    "edit tolerance",
    "cancel",
    "retry",
    "resolve as published",
    "resolve as failed",
];

/// A publication with its own channel and content, copying "Copy" from content
/// that says "Source". Its tolerance is 10 minutes, not the default.
async fn insert_publication(db: &PgPool, organization_id: &str, state: &str) -> String {
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
         VALUES ($1, $2, $3, $4, 'Copy', $5, '2026-12-01T09:00:00Z', 600,
             CASE WHEN $5 = 'published' THEN 'remote-1' END)",
    )
    .bind(&id)
    .bind(organization_id)
    .bind(&channel_id)
    .bind(&content_id)
    .bind(state)
    .execute(db)
    .await
    .unwrap();
    id
}

async fn set_state(db: &PgPool, publication_id: &str, state: &str) {
    sqlx::query("UPDATE pipewhere.publication SET state = $2 WHERE id = $1")
        .bind(publication_id)
        .bind(state)
        .execute(db)
        .await
        .unwrap();
}

fn new_key() -> String {
    uuid::Uuid::now_v7().simple().to_string()
}

fn path(organization_id: &str, publication_id: &str) -> String {
    format!("/v1/orgs/{organization_id}/publications/{publication_id}")
}

fn retry_request(
    instance: &Instance,
    organization_id: &str,
    token: &str,
    publication_id: &str,
    key: &str,
    body: Value,
) -> RequestBuilder {
    instance
        .request(
            Method::POST,
            &format!("{}/retry", path(organization_id, publication_id)),
            Some(token),
        )
        .header("idempotency-key", key)
        .json(&body)
}

async fn retry(
    instance: &Instance,
    organization_id: &str,
    token: &str,
    publication_id: &str,
    key: &str,
    body: Value,
) -> Response {
    retry_request(instance, organization_id, token, publication_id, key, body)
        .send()
        .await
        .unwrap()
}

async fn resolve(
    instance: &Instance,
    organization_id: &str,
    token: &str,
    publication_id: &str,
    body: Value,
) -> Response {
    instance
        .post(
            &format!("{}/resolve", path(organization_id, publication_id)),
            Some(token),
            body,
        )
        .await
}

/// A valid request for `action`, so that only the publication's state and
/// organization decide the answer.
fn action_request(
    instance: &Instance,
    organization_id: &str,
    token: &str,
    publication_id: &str,
    action: &str,
) -> RequestBuilder {
    let path = path(organization_id, publication_id);
    match action {
        "edit" => instance
            .request(Method::PATCH, &path, Some(token))
            .json(&json!({ "body": "Edited" })),
        "edit tolerance" => instance
            .request(Method::PATCH, &path, Some(token))
            .json(&json!({ "tolerance_seconds": 300 })),
        "cancel" => instance.request(Method::POST, &format!("{path}/cancel"), Some(token)),
        "retry" => retry_request(
            instance,
            organization_id,
            token,
            publication_id,
            &new_key(),
            json!({}),
        ),
        "resolve as published" => instance
            .request(Method::POST, &format!("{path}/resolve"), Some(token))
            .json(&json!({ "outcome": "published", "remote_id": "remote-2" })),
        "resolve as failed" => instance
            .request(Method::POST, &format!("{path}/resolve"), Some(token))
            .json(&json!({ "outcome": "failed" })),
        _ => unreachable!("{action}"),
    }
}

async fn act(
    instance: &Instance,
    organization_id: &str,
    token: &str,
    publication_id: &str,
    action: &str,
) -> Response {
    action_request(instance, organization_id, token, publication_id, action)
        .send()
        .await
        .unwrap()
}

async fn read(
    instance: &Instance,
    organization_id: &str,
    token: &str,
    publication_id: &str,
) -> Value {
    ok_json(
        instance
            .get(&path(organization_id, publication_id), Some(token))
            .await,
    )
    .await
}

async fn count(instance: &Instance, table: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT count(*) FROM pipewhere.{table}"
    )))
    .fetch_one(&instance.admin)
    .await
    .unwrap()
}

async fn now(instance: &Instance) -> DateTime<Utc> {
    sqlx::query_scalar("SELECT now()")
        .fetch_one(&instance.admin)
        .await
        .unwrap()
}

fn conflict() -> (StatusCode, String) {
    (StatusCode::CONFLICT, "state_conflict".to_owned())
}

#[tokio::test]
async fn each_action_is_allowed_only_in_the_states_d3_lists() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;

    for action in ACTIONS {
        let allowed: &[(&str, &str)] = match action {
            "edit" | "edit tolerance" => &[("scheduled", "scheduled"), ("failed", "failed")],
            "cancel" => &[("scheduled", "cancelled"), ("preparing", "cancelled")],
            "retry" => &[("failed", "scheduled")],
            "resolve as published" => &[("outcome_unknown", "published")],
            "resolve as failed" => &[("outcome_unknown", "failed")],
            _ => unreachable!(),
        };
        for from in STATES {
            let id = insert_publication(&instance.api_db, &organization_id, from).await;
            let before = read(&instance, &organization_id, &token, &id).await;
            let response = act(&instance, &organization_id, &token, &id, action).await;
            match allowed.iter().find(|(state, _)| *state == from) {
                Some((_, to)) => {
                    let after = ok_json(response).await;
                    assert_eq!(after["state"], *to, "{action} from {from}");
                    assert_eq!(
                        read(&instance, &organization_id, &token, &id).await,
                        after,
                        "{action} from {from}"
                    );
                }
                None => {
                    assert_eq!(refusal(response).await, conflict(), "{action} from {from}");
                    assert_eq!(
                        read(&instance, &organization_id, &token, &id).await,
                        before,
                        "{action} from {from} changes nothing"
                    );
                }
            }
        }
    }
    // Only the one retry that was allowed keeps its key.
    assert_eq!(count(&instance, "idempotency_key").await, 1);
    instance.close().await;
}

#[tokio::test]
async fn an_edit_changes_only_this_channels_copy() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;
    let id = insert_publication(&instance.api_db, &organization_id, "scheduled").await;
    let publication_path = path(&organization_id, &id);
    let original = read(&instance, &organization_id, &token, &id).await;

    let edited = ok_json(
        instance
            .patch(&publication_path, Some(&token), json!({ "body": "Edited" }))
            .await,
    )
    .await;
    let mut expected = original.clone();
    expected["body"] = json!("Edited");
    assert_eq!(edited, expected, "only the body changes");
    let content_path = format!(
        "/v1/orgs/{organization_id}/contents/{}",
        original["content_id"].as_str().unwrap()
    );
    assert_eq!(
        ok_json(instance.get(&content_path, Some(&token)).await).await["body"],
        "Source",
        "the content is not edited"
    );

    let retimed = ok_json(
        instance
            .patch(
                &publication_path,
                Some(&token),
                json!({ "planned_at": "2027-01-01T08:00:00+08:00" }),
            )
            .await,
    )
    .await;
    expected["planned_at"] = json!("2027-01-01T00:00:00Z");
    expected["deadline"] = json!("2027-01-01T00:10:00Z");
    assert_eq!(
        retimed, expected,
        "the planned time and its deadline change"
    );

    // 00003 D11: the tolerance is editable where the planned time is.
    for (tolerance, deadline) in [
        (300, "2027-01-01T00:05:00Z"),
        (86_400, "2027-01-02T00:00:00Z"),
    ] {
        let tolerated = ok_json(
            instance
                .patch(
                    &publication_path,
                    Some(&token),
                    json!({ "tolerance_seconds": tolerance }),
                )
                .await,
        )
        .await;
        expected["tolerance_seconds"] = json!(tolerance);
        expected["deadline"] = json!(deadline);
        assert_eq!(tolerated, expected, "the tolerance and its deadline change");
    }
    assert_eq!(
        ok_json(
            instance
                .patch(&publication_path, Some(&token), json!({}))
                .await
        )
        .await,
        expected,
        "an empty edit changes nothing"
    );

    let raw = async |body: String| {
        let response = instance
            .request(Method::PATCH, &publication_path, Some(&token))
            .header(CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .unwrap();
        refusal(response).await
    };
    let unprocessable = StatusCode::UNPROCESSABLE_ENTITY;
    let cases = [
        ("not JSON", "{".to_owned(), StatusCode::BAD_REQUEST),
        (
            "a body that is not text",
            json!({ "body": 1 }).to_string(),
            unprocessable,
        ),
        (
            "only whitespace",
            json!({ "body": " \n\t" }).to_string(),
            unprocessable,
        ),
        (
            "a NUL character, which PostgreSQL text cannot hold",
            json!({ "body": "a\u{0}b" }).to_string(),
            unprocessable,
        ),
        (
            "a channel, which is not editable",
            json!({ "channel_id": new_id("channel") }).to_string(),
            unprocessable,
        ),
        (
            "a state, which edit does not change",
            json!({ "state": "cancelled" }).to_string(),
            unprocessable,
        ),
        (
            "a planned time without an offset",
            json!({ "planned_at": "2027-01-01T00:00:00" }).to_string(),
            unprocessable,
        ),
        (
            "a planned time with single-digit fields",
            json!({ "planned_at": "2027-1-1T0:0:0Z" }).to_string(),
            unprocessable,
        ),
        (
            "a year 9999 time that is after 9999 in UTC",
            json!({ "planned_at": "9999-12-31T23:59:59-23:59" }).to_string(),
            unprocessable,
        ),
        (
            "a year 0000 time that is before 0000 in UTC",
            json!({ "planned_at": "0000-01-01T00:00:00+00:01" }).to_string(),
            unprocessable,
        ),
        (
            "a tolerance under 5 minutes",
            json!({ "tolerance_seconds": 299 }).to_string(),
            unprocessable,
        ),
        (
            "a tolerance over 24 hours",
            json!({ "tolerance_seconds": 86_401 }).to_string(),
            unprocessable,
        ),
        (
            "a negative tolerance",
            json!({ "tolerance_seconds": -1800 }).to_string(),
            unprocessable,
        ),
        (
            "a tolerance with a fraction of a second",
            json!({ "tolerance_seconds": 1800.5 }).to_string(),
            unprocessable,
        ),
        (
            "a tolerance that is not a number",
            json!({ "tolerance_seconds": "1800" }).to_string(),
            unprocessable,
        ),
    ];
    for (case, body, status) in cases {
        assert_eq!(
            raw(body).await,
            (status, "invalid_body".to_owned()),
            "{case}"
        );
    }
    assert_eq!(
        read(&instance, &organization_id, &token, &id).await,
        expected
    );
    instance.close().await;
}

#[tokio::test]
async fn a_repeated_retry_returns_the_publication_without_retrying_again() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;
    let first = insert_publication(&instance.api_db, &organization_id, "failed").await;
    let second = insert_publication(&instance.api_db, &organization_id, "failed").await;
    let key = new_key();
    let timed = json!({ "planned_at": "2027-01-01T08:00:00+08:00" });

    let retried = ok_json(
        retry(
            &instance,
            &organization_id,
            &token,
            &first,
            &key,
            timed.clone(),
        )
        .await,
    )
    .await;
    assert_eq!(retried["state"], "scheduled");
    assert_eq!(retried["planned_at"], "2027-01-01T00:00:00Z");
    // 00003 D11: a retry keeps the tolerance, and its new time moves the
    // deadline.
    assert_eq!(retried["tolerance_seconds"], 600);
    assert_eq!(retried["deadline"], "2027-01-01T00:10:00Z");
    assert_eq!(
        ok_json(
            retry(
                &instance,
                &organization_id,
                &token,
                &first,
                &key,
                timed.clone()
            )
            .await
        )
        .await,
        retried
    );

    // The worker fails it again. Repeating the request answers the publication
    // as it is now, and does not retry it a second time.
    set_state(&instance.api_db, &first, "failed").await;
    let again = ok_json(
        retry(
            &instance,
            &organization_id,
            &token,
            &first,
            &key,
            timed.clone(),
        )
        .await,
    )
    .await;
    assert_eq!(again["state"], "failed");
    assert_eq!(
        read(&instance, &organization_id, &token, &first).await,
        again
    );

    let reused = (
        StatusCode::UNPROCESSABLE_ENTITY,
        "idempotency_key_reused".to_owned(),
    );
    let create_key = new_key();
    let created = ok_json(
        instance
            .request(
                Method::POST,
                &format!("/v1/orgs/{organization_id}/publications"),
                Some(&token),
            )
            .header("idempotency-key", &create_key)
            .json(&json!({ "channel_id": retried["channel_id"], "body": "Created" }))
            .send()
            .await
            .unwrap(),
    )
    .await;
    let created = created["id"].as_str().unwrap();
    for (case, publication_id, key, body) in [
        (
            "another time",
            first.as_str(),
            &key,
            json!({ "planned_at": "2027-02-01T00:00:00Z" }),
        ),
        ("no time", &first, &key, json!({})),
        ("another publication", &second, &key, timed.clone()),
        (
            "a key that created a publication",
            created,
            &create_key,
            json!({}),
        ),
    ] {
        assert_eq!(
            refusal(
                retry(
                    &instance,
                    &organization_id,
                    &token,
                    publication_id,
                    key,
                    body
                )
                .await
            )
            .await,
            reused,
            "{case}"
        );
    }
    assert_eq!(
        read(&instance, &organization_id, &token, &second).await["state"],
        "failed"
    );

    // A refused retry does not use up its key.
    let refused_key = new_key();
    for (case, publication_id, expected) in [
        ("a publication in another state", created, conflict()),
        (
            "a publication that does not exist",
            new_id("publication").as_str(),
            (StatusCode::NOT_FOUND, "not_found".to_owned()),
        ),
    ] {
        assert_eq!(
            refusal(
                retry(
                    &instance,
                    &organization_id,
                    &token,
                    publication_id,
                    &refused_key,
                    json!({})
                )
                .await
            )
            .await,
            expected,
            "{case}"
        );
    }
    let before = now(&instance).await;
    let now_retried = ok_json(
        retry(
            &instance,
            &organization_id,
            &token,
            &second,
            &refused_key,
            json!({}),
        )
        .await,
    )
    .await;
    let after = now(&instance).await;
    let planned_at: DateTime<Utc> = now_retried["planned_at"].as_str().unwrap().parse().unwrap();
    assert!(
        before <= planned_at && planned_at <= after,
        "a retry without a time plans it for now: {before} <= {planned_at} <= {after}"
    );

    // Another organization has its own key space.
    let company = instance.create_organization(&owner, "company").await;
    let in_company = insert_publication(&instance.api_db, &company, "failed").await;
    let company_retried = ok_json(
        retry(
            &instance,
            &company,
            &token,
            &in_company,
            &key,
            timed.clone(),
        )
        .await,
    )
    .await;
    assert_eq!(company_retried["id"], in_company.as_str());
    assert_eq!(
        ok_json(
            retry(
                &instance,
                &company,
                &token,
                &in_company,
                &key,
                timed.clone()
            )
            .await
        )
        .await,
        company_retried
    );

    let invalid = (
        StatusCode::BAD_REQUEST,
        "invalid_idempotency_key".to_owned(),
    );
    let failed = insert_publication(&instance.api_db, &organization_id, "failed").await;
    let response = instance
        .post(
            &format!("{}/retry", path(&organization_id, &failed)),
            Some(&token),
            json!({}),
        )
        .await;
    assert_eq!(refusal(response).await, invalid, "no key");
    assert_eq!(
        refusal(
            retry(
                &instance,
                &organization_id,
                &token,
                &failed,
                &"a".repeat(21),
                json!({})
            )
            .await
        )
        .await,
        invalid,
        "21 characters"
    );
    for (case, body) in [
        (
            "an unknown field",
            json!({ "scheduled_at": "2027-01-01T00:00:00Z" }),
        ),
        (
            "a planned time without an offset",
            json!({ "planned_at": "2027-01-01T00:00:00" }),
        ),
        (
            "a year 9999 time that is after 9999 in UTC",
            json!({ "planned_at": "9999-12-31T23:59:59-23:59" }),
        ),
    ] {
        assert_eq!(
            refusal(
                retry(
                    &instance,
                    &organization_id,
                    &token,
                    &failed,
                    &new_key(),
                    body
                )
                .await
            )
            .await,
            (StatusCode::UNPROCESSABLE_ENTITY, "invalid_body".to_owned()),
            "{case}"
        );
    }
    assert_eq!(
        read(&instance, &organization_id, &token, &failed).await["state"],
        "failed"
    );
    // One create, and three retries: personal's two and company's one.
    assert_eq!(count(&instance, "idempotency_key").await, 4);
    instance.close().await;
}

#[tokio::test]
async fn resolving_records_what_a_person_found() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;

    let posted = insert_publication(&instance.api_db, &organization_id, "outcome_unknown").await;
    let unknown = read(&instance, &organization_id, &token, &posted).await;
    let invalid = (StatusCode::UNPROCESSABLE_ENTITY, "invalid_body".to_owned());
    for (case, body) in [
        ("no outcome", json!({ "remote_id": "109" })),
        (
            "an outcome D3 does not list",
            json!({ "outcome": "posted", "remote_id": "109" }),
        ),
        (
            "published without a remote id",
            json!({ "outcome": "published" }),
        ),
        (
            "a remote id that is only whitespace",
            json!({ "outcome": "published", "remote_id": " " }),
        ),
        (
            "a remote id holding a NUL character",
            json!({ "outcome": "published", "remote_id": "1\u{0}9" }),
        ),
        (
            "an address that is not a URL",
            json!({ "outcome": "published", "remote_id": "109", "remote_url": "mastodon.example/109" }),
        ),
        (
            "an address a link would run",
            json!({ "outcome": "published", "remote_id": "109", "remote_url": "javascript:alert(1)" }),
        ),
        (
            "failed with a remote id",
            json!({ "outcome": "failed", "remote_id": "109" }),
        ),
        (
            "failed with an address",
            json!({ "outcome": "failed", "remote_url": "https://mastodon.example/@me/109" }),
        ),
        (
            "an unknown field",
            json!({ "outcome": "failed", "state": "failed" }),
        ),
    ] {
        assert_eq!(
            refusal(resolve(&instance, &organization_id, &token, &posted, body).await).await,
            invalid,
            "{case}"
        );
    }
    assert_eq!(
        read(&instance, &organization_id, &token, &posted).await,
        unknown
    );

    let published = ok_json(
        resolve(
            &instance,
            &organization_id,
            &token,
            &posted,
            json!({ "outcome": "published", "remote_id": "109", "remote_url": "HTTPS://Mastodon.Example/@me/109" }),
        )
        .await,
    )
    .await;
    let mut expected = unknown.clone();
    expected["state"] = json!("published");
    expected["remote_id"] = json!("109");
    expected["remote_url"] = json!("https://mastodon.example/@me/109");
    assert_eq!(published, expected);

    // 00002 D2: nothing changes a published publication.
    for action in ACTIONS {
        assert_eq!(
            refusal(act(&instance, &organization_id, &token, &posted, action).await).await,
            conflict(),
            "{action}"
        );
    }
    assert_eq!(
        read(&instance, &organization_id, &token, &posted).await,
        expected
    );

    let without_url =
        insert_publication(&instance.api_db, &organization_id, "outcome_unknown").await;
    let published = ok_json(
        resolve(
            &instance,
            &organization_id,
            &token,
            &without_url,
            json!({ "outcome": "published", "remote_id": "110" }),
        )
        .await,
    )
    .await;
    assert_eq!(published["remote_id"], "110");
    assert_eq!(published["remote_url"], Value::Null);

    // A person who confirms nothing was posted can retry it.
    let missed = insert_publication(&instance.api_db, &organization_id, "outcome_unknown").await;
    let failed = ok_json(
        resolve(
            &instance,
            &organization_id,
            &token,
            &missed,
            json!({ "outcome": "failed" }),
        )
        .await,
    )
    .await;
    assert_eq!(failed["state"], "failed");
    assert_eq!(failed["remote_id"], Value::Null);
    let retried = ok_json(
        retry(
            &instance,
            &organization_id,
            &token,
            &missed,
            &new_key(),
            json!({}),
        )
        .await,
    )
    .await;
    assert_eq!(retried["state"], "scheduled");
    instance.close().await;
}

#[tokio::test]
async fn another_organizations_publication_is_not_found() {
    let instance = Instance::start().await;
    let user = instance.sign_up("user@example.com").await;
    let personal = instance.create_organization(&user, "personal").await;
    let company = instance.create_organization(&user, "company").await;
    let token = instance.session_token(&user).await;
    let outsider = instance.sign_up("outsider@example.com").await;
    let outsider_token = instance.session_token(&outsider).await;
    let not_found = (StatusCode::NOT_FOUND, "not_found".to_owned());

    for action in ACTIONS {
        // A state that allows the action, so only the organization refuses it.
        let state = match action {
            "edit" | "edit tolerance" | "cancel" => "scheduled",
            "retry" => "failed",
            _ => "outcome_unknown",
        };
        let company_publication = insert_publication(&instance.api_db, &company, state).await;
        let before = read(&instance, &company, &token, &company_publication).await;
        for (case, organization_id, token, publication_id) in [
            (
                "another organization's publication",
                &personal,
                &token,
                company_publication.as_str(),
            ),
            (
                "a publication that does not exist",
                &personal,
                &token,
                &new_id("publication"),
            ),
            (
                "an id that is not a publication id",
                &personal,
                &token,
                "not-an-id",
            ),
            (
                "an id holding a NUL character",
                &personal,
                &token,
                "bad%00id",
            ),
            (
                "a user who is not a member",
                &company,
                &outsider_token,
                company_publication.as_str(),
            ),
        ] {
            assert_eq!(
                refusal(act(&instance, organization_id, token, publication_id, action).await).await,
                not_found,
                "{action}: {case}"
            );
        }
        assert_eq!(
            read(&instance, &company, &token, &company_publication).await,
            before,
            "{action}"
        );
    }

    // A member's retry, repeated by a user who is not a member.
    let company_publication = insert_publication(&instance.api_db, &company, "failed").await;
    let key = new_key();
    ok_json(
        retry(
            &instance,
            &company,
            &token,
            &company_publication,
            &key,
            json!({}),
        )
        .await,
    )
    .await;
    assert_eq!(
        refusal(
            retry(
                &instance,
                &company,
                &outsider_token,
                &company_publication,
                &key,
                json!({})
            )
            .await
        )
        .await,
        not_found
    );
    assert_eq!(count(&instance, "idempotency_key").await, 1);
    instance.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_requests_change_a_publication_once() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;
    // Fetch Auth's signing keys first, so the requests do not queue behind
    // the first one's fetch.
    ok_json(
        instance
            .get(&format!("/v1/orgs/{organization_id}"), Some(&token))
            .await,
    )
    .await;

    let one_key = insert_publication(&instance.api_db, &organization_id, "failed").await;
    let key = new_key();
    let requests: Vec<_> = (0..20)
        .map(|_| {
            let request = retry_request(
                &instance,
                &organization_id,
                &token,
                &one_key,
                &key,
                json!({}),
            );
            tokio::spawn(request.send())
        })
        .collect();
    let mut answers = Vec::new();
    for request in requests {
        answers.push(ok_json(request.await.unwrap().unwrap()).await);
    }
    answers.dedup();
    assert_eq!(answers.len(), 1, "{answers:?}");
    assert_eq!(answers[0]["state"], "scheduled");

    for (action, from) in [("retry", "failed"), ("cancel", "scheduled")] {
        let id = insert_publication(&instance.api_db, &organization_id, from).await;
        let requests: Vec<_> = (0..20)
            .map(|_| {
                tokio::spawn(
                    action_request(&instance, &organization_id, &token, &id, action).send(),
                )
            })
            .collect();
        let mut statuses = Vec::new();
        for request in requests {
            statuses.push(request.await.unwrap().unwrap().status());
        }
        let changed = statuses.iter().filter(|status| status.is_success()).count();
        let refused = statuses
            .iter()
            .filter(|status| **status == StatusCode::CONFLICT)
            .count();
        assert_eq!((changed, refused), (1, 19), "{action}: {statuses:?}");
    }
    // The one-key retry and the one retry that changed its publication.
    assert_eq!(count(&instance, "idempotency_key").await, 2);
    instance.close().await;
}

// The concurrent test above races real requests, so whether two of them
// overlap is up to timing. Here a transaction holds the publication, as the
// worker claiming it would.
#[tokio::test]
async fn an_action_waits_for_another_transaction_changing_the_publication() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;

    for (action, from, claimed) in [
        ("edit", "scheduled", "preparing"),
        ("cancel", "preparing", "publishing"),
    ] {
        for commit in [true, false] {
            let id = insert_publication(&instance.api_db, &organization_id, from).await;
            let mut held = instance.api_db.begin().await.unwrap();
            sqlx::query("UPDATE pipewhere.publication SET state = $2 WHERE id = $1")
                .bind(&id)
                .bind(claimed)
                .execute(&mut *held)
                .await
                .unwrap();
            let request = tokio::spawn(
                action_request(&instance, &organization_id, &token, &id, action).send(),
            );
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            assert!(
                !request.is_finished(),
                "{action} waits for the publication's transaction"
            );
            if commit {
                held.commit().await.unwrap();
                assert_eq!(
                    refusal(request.await.unwrap().unwrap()).await,
                    conflict(),
                    "{action} after the held transaction moves it to {claimed}"
                );
                assert_eq!(
                    read(&instance, &organization_id, &token, &id).await["state"],
                    claimed
                );
            } else {
                held.rollback().await.unwrap();
                ok_json(request.await.unwrap().unwrap()).await;
            }
        }
    }
    instance.close().await;
}

#[tokio::test]
async fn a_retry_waits_for_another_transaction_holding_its_key() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;

    for commit in [true, false] {
        let id = insert_publication(&instance.api_db, &organization_id, "failed").await;
        let key = new_key();
        let mut held = instance.api_db.begin().await.unwrap();
        sqlx::query(
            "INSERT INTO pipewhere.idempotency_key (organization_id, key, request_hash, publication_id)
             VALUES ($1, $2, '\\x00', $3)",
        )
        .bind(&organization_id)
        .bind(&key)
        .bind(&id)
        .execute(&mut *held)
        .await
        .unwrap();

        let request = tokio::spawn(
            retry_request(&instance, &organization_id, &token, &id, &key, json!({})).send(),
        );
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        assert!(
            !request.is_finished(),
            "the retry waits for the key's transaction"
        );
        if commit {
            held.commit().await.unwrap();
            assert_eq!(
                refusal(request.await.unwrap().unwrap()).await,
                (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "idempotency_key_reused".to_owned()
                ),
                "the held transaction commits a different request"
            );
            assert_eq!(
                read(&instance, &organization_id, &token, &id).await["state"],
                "failed"
            );
        } else {
            held.rollback().await.unwrap();
            let retried = ok_json(request.await.unwrap().unwrap()).await;
            assert_eq!(
                retried["state"], "scheduled",
                "the held transaction rolls back"
            );
        }
    }
    instance.close().await;
}

#[tokio::test]
async fn a_published_publication_names_its_post() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let id = insert_publication(&instance.api_db, &organization_id, "scheduled").await;
    let constraint = async |state: &str, remote_id: Option<&str>| {
        sqlx::query("UPDATE pipewhere.publication SET state = $2, remote_id = $3 WHERE id = $1")
            .bind(&id)
            .bind(state)
            .bind(remote_id)
            .execute(&instance.api_db)
            .await
            .unwrap_err()
            .into_database_error()
            .unwrap()
            .constraint()
            .map(str::to_owned)
    };
    let check = Some("publication_remote_id_check".to_owned());
    assert_eq!(
        constraint("published", None).await,
        check,
        "published without a remote id"
    );
    assert_eq!(
        constraint("failed", Some("109")).await,
        check,
        "a remote id before published"
    );
    instance.close().await;
}
