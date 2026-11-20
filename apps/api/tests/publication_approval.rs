//! An agent's publications wait for a person to approve them, per 00010 D1 and
//! D4. API counts every API key as an agent.

mod support;

use pipewhere_api::ids::new_id;
use reqwest::{Method, Response, StatusCode};
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

async fn create(instance: &Instance, organization_id: &str, token: &str, body: Value) -> Value {
    let response = instance
        .request(
            Method::POST,
            &format!("/v1/orgs/{organization_id}/publications"),
            Some(token),
        )
        .header("idempotency-key", uuid::Uuid::now_v7().simple().to_string())
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    ok_json(response).await
}

async fn approve(
    instance: &Instance,
    organization_id: &str,
    token: &str,
    publication_id: &str,
) -> Response {
    instance
        .request(
            Method::POST,
            &format!("/v1/orgs/{organization_id}/publications/{publication_id}/approve"),
            Some(token),
        )
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn a_keys_publication_waits_until_a_member_approves_it() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;
    let key = instance.create_key(&owner, &organization_id).await;
    let key_token = instance.key_token(&key).await;
    let channel_id = insert_channel(&instance.api_db, &organization_id).await;
    let request = json!({ "channel_id": channel_id, "body": "Drafted", "planned_at": "2026-12-01T09:00:00Z" });

    let from_member = create(&instance, &organization_id, &token, request.clone()).await;
    assert_eq!(from_member["state"], "scheduled");

    let from_key = create(&instance, &organization_id, &key_token, request).await;
    assert_eq!(from_key["state"], "awaiting_approval");
    let id = from_key["id"].as_str().unwrap();

    // A key cannot approve, so an agent never approves its own work.
    assert_eq!(
        refusal(approve(&instance, &organization_id, &key_token, id).await).await,
        (StatusCode::FORBIDDEN, "forbidden".to_owned())
    );

    let approved = ok_json(approve(&instance, &organization_id, &token, id).await).await;
    let mut expected = from_key.clone();
    expected["state"] = json!("scheduled");
    assert_eq!(approved, expected);

    assert_eq!(
        refusal(approve(&instance, &organization_id, &token, id).await).await,
        (StatusCode::CONFLICT, "state_conflict".to_owned())
    );
    instance.close().await;
}

#[tokio::test]
async fn any_member_may_approve() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "team").await;
    let member = instance.sign_up("member@example.com").await;
    instance
        .add_member(
            &owner,
            &organization_id,
            &member,
            "member@example.com",
            "member",
        )
        .await;
    let member_token = instance.session_token(&member).await;
    let key = instance.create_key(&owner, &organization_id).await;
    let key_token = instance.key_token(&key).await;
    let channel_id = insert_channel(&instance.api_db, &organization_id).await;

    let waiting = create(
        &instance,
        &organization_id,
        &key_token,
        json!({ "channel_id": channel_id, "body": "Drafted" }),
    )
    .await;
    let approved = ok_json(
        approve(
            &instance,
            &organization_id,
            &member_token,
            waiting["id"].as_str().unwrap(),
        )
        .await,
    )
    .await;
    assert_eq!(approved["state"], "scheduled");
    instance.close().await;
}

async fn insert_publication(
    db: &PgPool,
    organization_id: &str,
    channel_id: &str,
    state: &str,
) -> String {
    let content_id = new_id("content");
    let id = new_id("publication");
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
             (id, organization_id, channel_id, content_id, body, state, planned_at, tolerance_seconds)
         VALUES ($1, $2, $3, $4, 'Approved', $5, '2026-12-01T09:00:00Z', 600)",
    )
    .bind(&id)
    .bind(organization_id)
    .bind(channel_id)
    .bind(&content_id)
    .bind(state)
    .execute(db)
    .await
    .unwrap();
    id
}

// A key could otherwise change text a member approved, send a failed
// publication out again without a member seeing it, or stop a member's post.
#[tokio::test]
async fn a_key_cannot_change_a_publication_beyond_withdrawing_a_draft() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;
    let key = instance.create_key(&owner, &organization_id).await;
    let key_token = instance.key_token(&key).await;
    let channel_id = insert_channel(&instance.api_db, &organization_id).await;
    let forbidden = (StatusCode::FORBIDDEN, "forbidden".to_owned());
    let path = |id: &str| format!("/v1/orgs/{organization_id}/publications/{id}");

    for (state, method, action, body) in [
        (
            "awaiting_approval",
            Method::PATCH,
            "",
            json!({ "body": "Changed" }),
        ),
        ("scheduled", Method::PATCH, "", json!({ "body": "Changed" })),
        ("failed", Method::PATCH, "", json!({ "body": "Changed" })),
        ("failed", Method::POST, "/retry", json!({})),
        ("scheduled", Method::POST, "/cancel", json!({})),
        ("preparing", Method::POST, "/cancel", json!({})),
        (
            "outcome_unknown",
            Method::POST,
            "/resolve",
            json!({ "outcome": "failed" }),
        ),
    ] {
        let id = insert_publication(&instance.api_db, &organization_id, &channel_id, state).await;
        let before = ok_json(instance.get(&path(&id), Some(&token)).await).await;
        let response = instance
            .request(
                method.clone(),
                &format!("{}{action}", path(&id)),
                Some(&key_token),
            )
            .header("idempotency-key", uuid::Uuid::now_v7().simple().to_string())
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(
            refusal(response).await,
            forbidden,
            "{method} {action} from {state}"
        );
        assert_eq!(
            ok_json(instance.get(&path(&id), Some(&token)).await).await,
            before,
            "{method} {action} from {state} changes nothing"
        );
    }
    instance.close().await;
}

#[tokio::test]
async fn a_key_may_withdraw_its_waiting_publication() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let key = instance.create_key(&owner, &organization_id).await;
    let key_token = instance.key_token(&key).await;
    let channel_id = insert_channel(&instance.api_db, &organization_id).await;

    let waiting = create(
        &instance,
        &organization_id,
        &key_token,
        json!({ "channel_id": channel_id, "body": "Drafted" }),
    )
    .await;
    let id = waiting["id"].as_str().unwrap();
    let cancelled = ok_json(
        instance
            .post(
                &format!("/v1/orgs/{organization_id}/publications/{id}/cancel"),
                Some(&key_token),
                json!({}),
            )
            .await,
    )
    .await;
    assert_eq!(cancelled["state"], "cancelled");
    instance.close().await;
}
