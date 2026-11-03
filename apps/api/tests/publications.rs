//! Creating and reading publications, per 00002 D2, D3, D5, D6, and D11–D14,
//! and 00003 D11. Connecting a channel belongs to 00004, so these tests insert
//! channel rows directly.

mod support;

use chrono::{DateTime, Utc};
use pipewhere_api::ids::new_id;
use reqwest::header::CONTENT_TYPE;
use reqwest::{Method, RequestBuilder, Response, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Instance, ok_json, refusal};

async fn insert_channel(db: &PgPool, organization_id: &str) -> String {
    let id = new_id("channel");
    sqlx::query(
        "INSERT INTO pipewhere.channel (id, organization_id, provider) VALUES ($1, $2, 'mastodon')",
    )
    .bind(&id)
    .bind(organization_id)
    .execute(db)
    .await
    .unwrap();
    id
}

fn new_key() -> String {
    uuid::Uuid::now_v7().simple().to_string()
}

fn create_request(
    instance: &Instance,
    organization_id: &str,
    token: &str,
    key: &str,
    body: Value,
) -> RequestBuilder {
    instance
        .request(
            Method::POST,
            &format!("/v1/orgs/{organization_id}/publications"),
            Some(token),
        )
        .header("idempotency-key", key)
        .json(&body)
}

async fn create(
    instance: &Instance,
    organization_id: &str,
    token: &str,
    key: &str,
    body: Value,
) -> Response {
    create_request(instance, organization_id, token, key, body)
        .send()
        .await
        .unwrap()
}

async fn created(response: Response) -> Value {
    assert_eq!(response.status(), StatusCode::CREATED);
    ok_json(response).await
}

async fn count(instance: &Instance, table: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT count(*) FROM pipewhere.{table}"
    )))
    .fetch_one(&instance.admin)
    .await
    .unwrap()
}

#[tokio::test]
async fn a_member_creates_lists_and_reads_publications() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;
    let channel_id = insert_channel(&instance.api_db, &organization_id).await;
    let publications = format!("/v1/orgs/{organization_id}/publications");

    let from_text = created(
        create(
            &instance,
            &organization_id,
            &token,
            &new_key(),
            json!({
                "channel_id": channel_id,
                "body": "Hello",
                "planned_at": "2026-12-01T17:00:00+08:00",
            }),
        )
        .await,
    )
    .await;
    let id = from_text["id"].as_str().unwrap().to_owned();
    let content_id = from_text["content_id"].as_str().unwrap().to_owned();
    assert!(id.starts_with("pub_"), "{id}");
    assert_eq!(
        from_text,
        json!({
            "id": id,
            "organization_id": organization_id,
            "channel_id": channel_id,
            "content_id": content_id,
            "body": "Hello",
            "state": "scheduled",
            "planned_at": "2026-12-01T09:00:00Z",
            "tolerance_seconds": 1800,
            "deadline": "2026-12-01T09:30:00Z",
            "remote_id": null,
            "remote_url": null,
        })
    );
    let content_path = format!("/v1/orgs/{organization_id}/contents/{content_id}");
    let content = ok_json(instance.get(&content_path, Some(&token)).await).await;
    assert_eq!(content["body"], "Hello");

    // Editing the content leaves the publication's copy alone, and a new
    // publication made from it copies the edited text.
    let edit = json!({ "body": "Hello, edited" });
    ok_json(instance.patch(&content_path, Some(&token), edit).await).await;
    let before: DateTime<Utc> = sqlx::query_scalar("SELECT now()")
        .fetch_one(&instance.admin)
        .await
        .unwrap();
    let from_content = created(
        create(
            &instance,
            &organization_id,
            &token,
            &new_key(),
            json!({ "channel_id": channel_id, "content_id": content_id }),
        )
        .await,
    )
    .await;
    let after: DateTime<Utc> = sqlx::query_scalar("SELECT now()")
        .fetch_one(&instance.admin)
        .await
        .unwrap();
    assert_eq!(from_content["content_id"], content_id);
    assert_eq!(from_content["body"], "Hello, edited");
    let planned_at: DateTime<Utc> = from_content["planned_at"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        before <= planned_at && planned_at <= after,
        "{before} <= {planned_at} <= {after}"
    );

    assert_eq!(
        ok_json(
            instance
                .get(&format!("{publications}/{id}"), Some(&token))
                .await
        )
        .await,
        from_text
    );
    assert_eq!(
        ok_json(instance.get(&publications, Some(&token)).await).await,
        json!({ "publications": [from_content, from_text], "next_cursor": null })
    );
    instance.close().await;
}

#[tokio::test]
async fn a_publication_records_its_tolerance() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;
    let channel_id = insert_channel(&instance.api_db, &organization_id).await;

    for (tolerance, deadline) in [
        (300, "2026-12-01T09:05:00Z"),
        (86_400, "2026-12-02T09:00:00Z"),
    ] {
        let publication = created(
            create(
                &instance,
                &organization_id,
                &token,
                &new_key(),
                json!({
                    "channel_id": channel_id,
                    "body": "Hello",
                    "planned_at": "2026-12-01T09:00:00Z",
                    "tolerance_seconds": tolerance,
                }),
            )
            .await,
        )
        .await;
        assert_eq!(publication["tolerance_seconds"], tolerance);
        assert_eq!(publication["deadline"], deadline);
    }

    // Publishing now takes the same rule, from the time of the request.
    let now = created(
        create(
            &instance,
            &organization_id,
            &token,
            &new_key(),
            json!({ "channel_id": channel_id, "body": "Now" }),
        )
        .await,
    )
    .await;
    let planned_at: DateTime<Utc> = now["planned_at"].as_str().unwrap().parse().unwrap();
    let deadline: DateTime<Utc> = now["deadline"].as_str().unwrap().parse().unwrap();
    assert_eq!(now["tolerance_seconds"], 1800);
    assert_eq!(deadline - planned_at, chrono::TimeDelta::minutes(30));
    instance.close().await;
}

#[tokio::test]
async fn a_repeated_request_returns_the_first_result() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;
    let key = instance.create_key(&owner, &organization_id).await;
    let key_token = instance.key_token(&key).await;
    let channel_id = insert_channel(&instance.api_db, &organization_id).await;
    let idempotency_key = new_key();
    let now = json!({ "channel_id": channel_id, "body": "Once" });

    let first = created(
        create(
            &instance,
            &organization_id,
            &token,
            &idempotency_key,
            now.clone(),
        )
        .await,
    )
    .await;
    let again = created(
        create(
            &instance,
            &organization_id,
            &token,
            &idempotency_key,
            now.clone(),
        )
        .await,
    )
    .await;
    assert_eq!(
        again, first,
        "a request to publish now keeps its first time"
    );

    // The key belongs to the organization, not to the actor that sent it.
    let from_key = created(
        create(
            &instance,
            &organization_id,
            &key_token,
            &idempotency_key,
            now.clone(),
        )
        .await,
    )
    .await;
    assert_eq!(from_key, first);

    let reused = (
        StatusCode::UNPROCESSABLE_ENTITY,
        "idempotency_key_reused".to_owned(),
    );
    for (case, body) in [
        (
            "other text",
            json!({ "channel_id": channel_id, "body": "Twice" }),
        ),
        (
            "a planned time",
            json!({ "channel_id": channel_id, "body": "Once", "planned_at": "2026-12-01T09:00:00Z" }),
        ),
        (
            "existing content",
            json!({ "channel_id": channel_id, "content_id": first["content_id"] }),
        ),
        (
            "a tolerance, even the default one",
            json!({ "channel_id": channel_id, "body": "Once", "tolerance_seconds": 1800 }),
        ),
    ] {
        assert_eq!(
            refusal(create(&instance, &organization_id, &token, &idempotency_key, body).await)
                .await,
            reused,
            "{case}"
        );
    }

    // Whitespace, field order, and a timestamp's offset do not make a request
    // different.
    let timed_key = new_key();
    let timed = created(
        create(
            &instance,
            &organization_id,
            &token,
            &timed_key,
            json!({ "channel_id": channel_id, "body": "Timed", "planned_at": "2026-12-01T09:00:00Z" }),
        )
        .await,
    )
    .await;
    let response = instance
        .request(
            Method::POST,
            &format!("/v1/orgs/{organization_id}/publications"),
            Some(&token),
        )
        .header("idempotency-key", &timed_key)
        .header(CONTENT_TYPE, "application/json")
        .body(format!(
            r#"{{ "planned_at": "2026-12-01T17:00:00+08:00", "body": "Timed", "channel_id": "{channel_id}" }}"#
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(created(response).await, timed);

    // Another organization has its own key space.
    let company = instance.create_organization(&owner, "company").await;
    let company_channel = insert_channel(&instance.api_db, &company).await;
    let company_request = json!({ "channel_id": company_channel, "body": "Once" });
    let in_company = created(
        create(
            &instance,
            &company,
            &token,
            &idempotency_key,
            company_request.clone(),
        )
        .await,
    )
    .await;
    assert_ne!(in_company["id"], first["id"]);
    assert_eq!(in_company["organization_id"], company);
    assert_eq!(
        created(
            create(
                &instance,
                &company,
                &token,
                &idempotency_key,
                company_request
            )
            .await
        )
        .await,
        in_company
    );
    assert_eq!(
        created(create(&instance, &organization_id, &token, &idempotency_key, now).await).await,
        first
    );

    assert_eq!(count(&instance, "publication").await, 3);
    assert_eq!(count(&instance, "content").await, 3);
    instance.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_use_of_one_key_creates_one_publication() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;
    let channel_id = insert_channel(&instance.api_db, &organization_id).await;
    let idempotency_key = new_key();
    let body = json!({ "channel_id": channel_id, "body": "Once" });
    // Fetch Auth's signing keys first, so the requests do not queue behind
    // the first one's fetch.
    ok_json(
        instance
            .get(&format!("/v1/orgs/{organization_id}"), Some(&token))
            .await,
    )
    .await;

    let requests: Vec<_> = (0..20)
        .map(|_| {
            let request = create_request(
                &instance,
                &organization_id,
                &token,
                &idempotency_key,
                body.clone(),
            );
            tokio::spawn(request.send())
        })
        .collect();
    let mut ids = Vec::new();
    for request in requests {
        ids.push(created(request.await.unwrap().unwrap()).await["id"].clone());
    }
    ids.dedup();
    assert_eq!(ids.len(), 1, "{ids:?}");
    assert_eq!(count(&instance, "publication").await, 1);
    assert_eq!(count(&instance, "content").await, 1);
    instance.close().await;
}

// The concurrent test above races real requests, so whether two of them
// overlap is up to timing. Here a transaction holds the key, as a first
// request still in flight would.
#[tokio::test]
async fn a_request_waits_for_another_transaction_holding_its_key() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;
    let channel_id = insert_channel(&instance.api_db, &organization_id).await;
    let body = json!({ "channel_id": channel_id, "body": "Second" });

    for commit in [true, false] {
        let key = new_key();
        let mut held = instance.api_db.begin().await.unwrap();
        let content_id = new_id("content");
        let publication_id = new_id("publication");
        sqlx::query(
            "INSERT INTO pipewhere.idempotency_key (organization_id, key, request_hash, publication_id)
             VALUES ($1, $2, '\\x00', $3)",
        )
        .bind(&organization_id)
        .bind(&key)
        .bind(&publication_id)
        .execute(&mut *held)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pipewhere.content (id, organization_id, body) VALUES ($1, $2, 'First')",
        )
        .bind(&content_id)
        .bind(&organization_id)
        .execute(&mut *held)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO pipewhere.publication
                 (id, organization_id, channel_id, content_id, body, state, planned_at, tolerance_seconds)
             VALUES ($1, $2, $3, $4, 'First', 'scheduled', now(), 1800)",
        )
        .bind(&publication_id)
        .bind(&organization_id)
        .bind(&channel_id)
        .bind(&content_id)
        .execute(&mut *held)
        .await
        .unwrap();

        let request = tokio::spawn(
            create_request(&instance, &organization_id, &token, &key, body.clone()).send(),
        );
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        assert!(
            !request.is_finished(),
            "the request waits for the key's transaction"
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
        } else {
            held.rollback().await.unwrap();
            let created = created(request.await.unwrap().unwrap()).await;
            assert_eq!(created["body"], "Second", "the held transaction rolls back");
        }
    }
    assert_eq!(count(&instance, "publication").await, 2);
    instance.close().await;
}

#[tokio::test]
async fn a_missing_or_short_key_is_refused() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;
    let channel_id = insert_channel(&instance.api_db, &organization_id).await;
    let body = json!({ "channel_id": channel_id, "body": "Hello" });
    let invalid = (
        StatusCode::BAD_REQUEST,
        "invalid_idempotency_key".to_owned(),
    );

    let response = instance
        .post(
            &format!("/v1/orgs/{organization_id}/publications"),
            Some(&token),
            body.clone(),
        )
        .await;
    assert_eq!(refusal(response).await, invalid, "no key");
    for (case, key) in [
        ("21 characters", "a".repeat(21)),
        ("256 characters", "a".repeat(256)),
    ] {
        assert_eq!(
            refusal(create(&instance, &organization_id, &token, &key, body.clone()).await).await,
            invalid,
            "{case}"
        );
    }
    let response = instance
        .request(
            Method::POST,
            &format!("/v1/orgs/{organization_id}/publications"),
            Some(&token),
        )
        .header(
            "idempotency-key",
            reqwest::header::HeaderValue::from_bytes("é".repeat(15).as_bytes()).unwrap(),
        )
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(
        refusal(response).await,
        invalid,
        "30 bytes that are not visible ASCII"
    );
    for key in ["a".repeat(22), "a".repeat(255)] {
        created(create(&instance, &organization_id, &token, &key, body.clone()).await).await;
    }
    assert_eq!(count(&instance, "publication").await, 2);
    instance.close().await;
}

#[tokio::test]
async fn another_organizations_resources_are_not_found() {
    let instance = Instance::start().await;
    let user = instance.sign_up("user@example.com").await;
    let personal = instance.create_organization(&user, "personal").await;
    let company = instance.create_organization(&user, "company").await;
    let token = instance.session_token(&user).await;
    let personal_channel = insert_channel(&instance.api_db, &personal).await;
    let company_channel = insert_channel(&instance.api_db, &company).await;
    let company_key = new_key();
    let company_request = json!({ "channel_id": company_channel, "body": "Company writing" });
    let company_publication = created(
        create(
            &instance,
            &company,
            &token,
            &company_key,
            company_request.clone(),
        )
        .await,
    )
    .await;
    let company_content = company_publication["content_id"].as_str().unwrap();

    let not_found = (StatusCode::NOT_FOUND, "not_found".to_owned());
    for (case, publication_id) in [
        (
            "another organization's publication",
            company_publication["id"].as_str().unwrap().to_owned(),
        ),
        ("a publication that does not exist", new_id("publication")),
        ("an id that is not a publication id", "not-an-id".to_owned()),
        ("an id holding a NUL character", "bad%00id".to_owned()),
    ] {
        let path = format!("/v1/orgs/{personal}/publications/{publication_id}");
        assert_eq!(
            refusal(instance.get(&path, Some(&token)).await).await,
            not_found,
            "{case}"
        );
    }
    assert_eq!(
        ok_json(
            instance
                .get(&format!("/v1/orgs/{personal}/publications"), Some(&token))
                .await
        )
        .await,
        json!({ "publications": [], "next_cursor": null })
    );
    assert_eq!(
        ok_json(
            instance
                .get(
                    &format!("/v1/orgs/{personal}/publications?cursor=bad%00cursor"),
                    Some(&token)
                )
                .await
        )
        .await,
        json!({ "publications": [], "next_cursor": null }),
        "a cursor holding a NUL character"
    );

    let channel_not_found = (
        StatusCode::UNPROCESSABLE_ENTITY,
        "channel_not_found".to_owned(),
    );
    let content_not_found = (
        StatusCode::UNPROCESSABLE_ENTITY,
        "content_not_found".to_owned(),
    );
    let key = new_key();
    for (case, body, expected) in [
        (
            "another organization's channel",
            json!({ "channel_id": company_channel, "body": "Hello" }),
            &channel_not_found,
        ),
        (
            "a channel that does not exist",
            json!({ "channel_id": new_id("channel"), "body": "Hello" }),
            &channel_not_found,
        ),
        (
            "another organization's content",
            json!({ "channel_id": personal_channel, "content_id": company_content }),
            &content_not_found,
        ),
        (
            "content that does not exist",
            json!({ "channel_id": personal_channel, "content_id": new_id("content") }),
            &content_not_found,
        ),
        (
            "a channel id holding a NUL character",
            json!({ "channel_id": "bad\u{0}channel", "body": "Hello" }),
            &channel_not_found,
        ),
        (
            "a content id holding a NUL character",
            json!({ "channel_id": personal_channel, "content_id": "bad\u{0}content" }),
            &content_not_found,
        ),
    ] {
        assert_eq!(
            &refusal(create(&instance, &personal, &token, &key, body).await).await,
            expected,
            "{case}"
        );
    }
    // A refused request does not use up its key.
    created(
        create(
            &instance,
            &personal,
            &token,
            &key,
            json!({ "channel_id": personal_channel, "body": "Hello" }),
        )
        .await,
    )
    .await;

    let outsider = instance.sign_up("outsider@example.com").await;
    let outsider_token = instance.session_token(&outsider).await;
    let publications = format!("/v1/orgs/{company}/publications");
    assert_eq!(
        refusal(instance.get(&publications, Some(&outsider_token)).await).await,
        not_found,
        "a user who is not a member lists"
    );
    assert_eq!(
        refusal(
            instance
                .get(
                    &format!(
                        "{publications}/{}",
                        company_publication["id"].as_str().unwrap()
                    ),
                    Some(&outsider_token)
                )
                .await
        )
        .await,
        not_found,
        "a user who is not a member reads"
    );
    assert_eq!(
        refusal(
            create(
                &instance,
                &company,
                &outsider_token,
                &new_key(),
                json!({ "channel_id": company_channel, "body": "Hello" }),
            )
            .await
        )
        .await,
        not_found,
        "a user who is not a member creates"
    );
    assert_eq!(
        refusal(
            create(
                &instance,
                &company,
                &outsider_token,
                &company_key,
                company_request,
            )
            .await
        )
        .await,
        not_found,
        "a user who is not a member repeats a member's request"
    );

    assert_eq!(count(&instance, "publication").await, 2);
    assert_eq!(count(&instance, "content").await, 2);
    instance.close().await;
}

#[tokio::test]
async fn a_body_api_cannot_read_is_refused() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;
    let channel_id = insert_channel(&instance.api_db, &organization_id).await;
    let content_id = ok_json(
        instance
            .post(
                &format!("/v1/orgs/{organization_id}/contents"),
                Some(&token),
                json!({ "body": "Kept" }),
            )
            .await,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let raw = async |body: String| {
        let response = instance
            .request(
                Method::POST,
                &format!("/v1/orgs/{organization_id}/publications"),
                Some(&token),
            )
            .header("idempotency-key", new_key())
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
        ("no channel", json!({ "body": "Hello" }).to_string(), unprocessable),
        ("neither body nor content", json!({ "channel_id": channel_id }).to_string(), unprocessable),
        (
            "both body and content",
            json!({ "channel_id": channel_id, "body": "Hello", "content_id": content_id }).to_string(),
            unprocessable,
        ),
        ("only whitespace", json!({ "channel_id": channel_id, "body": " \n\t" }).to_string(), unprocessable),
        (
            "a NUL character, which PostgreSQL text cannot hold",
            json!({ "channel_id": channel_id, "body": "a\u{0}b" }).to_string(),
            unprocessable,
        ),
        (
            "a planned time without an offset",
            json!({ "channel_id": channel_id, "body": "Hello", "planned_at": "2026-12-01T09:00:00" }).to_string(),
            unprocessable,
        ),
        (
            "a misspelt planned time, which would otherwise plan it for now",
            json!({ "channel_id": channel_id, "body": "Hello", "scheduled_at": "2026-12-01T09:00:00Z" })
                .to_string(),
            unprocessable,
        ),
        (
            "a year with five digits, which RFC 3339 cannot write",
            json!({ "channel_id": channel_id, "body": "Hello", "planned_at": "+10000-01-01T00:00:00Z" })
                .to_string(),
            unprocessable,
        ),
        (
            "a year 9999 time that is after 9999 in UTC",
            json!({ "channel_id": channel_id, "body": "Hello", "planned_at": "9999-12-31T23:59:59-23:59" })
                .to_string(),
            unprocessable,
        ),
        (
            "a leap second that carries into 10000 when stored",
            json!({ "channel_id": channel_id, "body": "Hello", "planned_at": "9999-12-31T23:59:60Z" })
                .to_string(),
            unprocessable,
        ),
        (
            "a year 0000 time that is before 0000 in UTC",
            json!({ "channel_id": channel_id, "body": "Hello", "planned_at": "0000-01-01T00:00:00+00:01" })
                .to_string(),
            unprocessable,
        ),
        (
            "a planned time with single-digit fields",
            json!({ "channel_id": channel_id, "body": "Hello", "planned_at": "2026-1-1T9:0:0Z" })
                .to_string(),
            unprocessable,
        ),
        (
            "a planned time with an offset that has no colon",
            json!({ "channel_id": channel_id, "body": "Hello", "planned_at": "2026-12-01T09:00:00+0800" })
                .to_string(),
            unprocessable,
        ),
        (
            "a planned time named in UTC rather than offset",
            json!({ "channel_id": channel_id, "body": "Hello", "planned_at": "2026-12-01T09:00:00UTC" })
                .to_string(),
            unprocessable,
        ),
        (
            "a planned time that is not a time",
            json!({ "channel_id": channel_id, "body": "Hello", "planned_at": "tomorrow" }).to_string(),
            unprocessable,
        ),
        (
            "a tolerance under 5 minutes",
            json!({ "channel_id": channel_id, "body": "Hello", "tolerance_seconds": 299 }).to_string(),
            unprocessable,
        ),
        (
            "a tolerance over 24 hours",
            json!({ "channel_id": channel_id, "body": "Hello", "tolerance_seconds": 86_401 }).to_string(),
            unprocessable,
        ),
        (
            "a tolerance of nothing",
            json!({ "channel_id": channel_id, "body": "Hello", "tolerance_seconds": 0 }).to_string(),
            unprocessable,
        ),
        (
            "a tolerance too large for the column",
            json!({ "channel_id": channel_id, "body": "Hello", "tolerance_seconds": 4_294_967_296_u64 })
                .to_string(),
            unprocessable,
        ),
        (
            "a tolerance in minutes, as text",
            json!({ "channel_id": channel_id, "body": "Hello", "tolerance_seconds": "30m" }).to_string(),
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

    assert_eq!(count(&instance, "publication").await, 0);
    assert_eq!(count(&instance, "content").await, 1);
    assert_eq!(count(&instance, "idempotency_key").await, 0);
    instance.close().await;
}

#[tokio::test]
async fn the_list_pages_by_cursor() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;
    let channel_id = insert_channel(&instance.api_db, &organization_id).await;
    let content_id = new_id("content");
    sqlx::query("INSERT INTO pipewhere.content (id, organization_id, body) VALUES ($1, $2, 'x')")
        .bind(&content_id)
        .bind(&organization_id)
        .execute(&instance.api_db)
        .await
        .unwrap();
    let mut ids: Vec<String> = (0..150).map(|_| new_id("publication")).collect();
    sqlx::query(
        "INSERT INTO pipewhere.publication
             (id, organization_id, channel_id, content_id, body, state, planned_at, tolerance_seconds)
         SELECT id, $2, $3, $4, 'x', 'scheduled', now(), 1800 FROM unnest($1::text[]) AS id",
    )
    .bind(&ids)
    .bind(&organization_id)
    .bind(&channel_id)
    .bind(&content_id)
    .execute(&instance.api_db)
    .await
    .unwrap();
    ids.sort();
    ids.reverse();

    let publications = format!("/v1/orgs/{organization_id}/publications");
    let page_ids = |page: &Value| -> Vec<String> {
        page["publications"]
            .as_array()
            .unwrap()
            .iter()
            .map(|publication| publication["id"].as_str().unwrap().to_owned())
            .collect()
    };
    let first = ok_json(instance.get(&publications, Some(&token)).await).await;
    assert_eq!(page_ids(&first), ids[..100]);
    let cursor = first["next_cursor"].as_str().unwrap();
    let second = ok_json(
        instance
            .get(&format!("{publications}?cursor={cursor}"), Some(&token))
            .await,
    )
    .await;
    assert_eq!(page_ids(&second), ids[100..]);
    assert_eq!(second["next_cursor"], Value::Null);
    instance.close().await;
}

#[tokio::test]
async fn the_tables_refuse_bad_rows() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;
    let channel_id = insert_channel(&instance.api_db, &organization_id).await;
    let publication = created(
        create(
            &instance,
            &organization_id,
            &token,
            &new_key(),
            json!({ "channel_id": channel_id, "body": "Kept" }),
        )
        .await,
    )
    .await;
    let content_id = publication["content_id"].as_str().unwrap();
    let insert = async |id: &str, organization_id: &str, state: &str, tolerance: i32| {
        sqlx::query(
            "INSERT INTO pipewhere.publication
                 (id, organization_id, channel_id, content_id, body, state, planned_at, tolerance_seconds)
             VALUES ($1, $2, $3, $4, 'x', $5, now(), $6)",
        )
        .bind(id)
        .bind(organization_id)
        .bind(&channel_id)
        .bind(content_id)
        .bind(state)
        .bind(tolerance)
        .execute(&instance.api_db)
        .await
        .unwrap_err()
        .into_database_error()
        .unwrap()
        .constraint()
        .map(str::to_owned)
    };
    assert_eq!(
        insert(&new_id("content"), &organization_id, "scheduled", 1800).await,
        Some("publication_id_check".into())
    );
    assert_eq!(
        insert(&new_id("publication"), &organization_id, "posted", 1800).await,
        Some("publication_state_check".into())
    );
    assert_eq!(
        insert(
            &new_id("publication"),
            "org_01m36mep00e3ca12pwd1gkjwex",
            "scheduled",
            1800
        )
        .await,
        Some("publication_organization_id_fkey".into())
    );
    // 00003 D1: from 5 minutes to 24 hours.
    for tolerance in [299, 86_401] {
        assert_eq!(
            insert(
                &new_id("publication"),
                &organization_id,
                "scheduled",
                tolerance
            )
            .await,
            Some("publication_tolerance_seconds_check".into()),
            "{tolerance}"
        );
    }
    // A writer that leaves the tolerance out fails rather than taking a
    // default.
    let error = sqlx::query(
        "INSERT INTO pipewhere.publication (id, organization_id, channel_id, content_id, body, state, planned_at)
         VALUES ($1, $2, $3, $4, 'x', 'scheduled', now())",
    )
    .bind(new_id("publication"))
    .bind(&organization_id)
    .bind(&channel_id)
    .bind(content_id)
    .execute(&instance.api_db)
    .await
    .unwrap_err();
    assert_eq!(
        error.into_database_error().unwrap().code().as_deref(),
        Some("23502"),
        "not_null_violation"
    );

    // A key whose publication never arrives fails at commit.
    let error = sqlx::query(
        "INSERT INTO pipewhere.idempotency_key (organization_id, key, request_hash, publication_id)
         VALUES ($1, $2, '\\x00', $3)",
    )
    .bind(&organization_id)
    .bind(new_key())
    .bind(new_id("publication"))
    .execute(&instance.api_db)
    .await
    .unwrap_err();
    assert_eq!(
        error.into_database_error().unwrap().constraint(),
        Some("idempotency_key_publication_id_fkey")
    );

    // 00001 D2: a foreign key to an organization never cascades.
    let actions: Vec<(String, String)> = sqlx::query_as(
        "SELECT conname::text, confdeltype::text FROM pg_constraint
         WHERE confrelid = 'auth.organization'::regclass AND conrelid::regclass::text IN
             ('pipewhere.publication', 'pipewhere.idempotency_key')
         ORDER BY conname",
    )
    .fetch_all(&instance.admin)
    .await
    .unwrap();
    assert_eq!(
        actions,
        [
            (
                "idempotency_key_organization_id_fkey".to_owned(),
                "a".to_owned()
            ),
            (
                "publication_organization_id_fkey".to_owned(),
                "a".to_owned()
            ),
        ]
    );
    instance.close().await;
}
