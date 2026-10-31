//! 00001 D2's checks that need API, against a real Auth.

mod support;

use reqwest::StatusCode;
use serde_json::json;
use support::{Instance, ok_json, refusal};

fn org_path(organization_id: &str) -> String {
    format!("/v1/orgs/{organization_id}")
}

#[tokio::test]
async fn health_needs_no_token() {
    let instance = Instance::start().await;
    let response = instance.get("/health", None).await;
    assert_eq!(response.status(), StatusCode::OK);
    instance.close().await;
}

#[tokio::test]
async fn unknown_paths_and_methods_answer_with_a_code() {
    let instance = Instance::start().await;
    let unknown_path = instance.get("/nope", None).await;
    assert_eq!(
        refusal(unknown_path).await,
        (StatusCode::NOT_FOUND, "not_found".into())
    );
    let wrong_method = instance
        .http
        .post(format!("{}/health", instance.api_url))
        .send()
        .await
        .unwrap();
    assert_eq!(
        refusal(wrong_method).await,
        (StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed".into())
    );
    instance.close().await;
}

#[tokio::test]
async fn a_member_reads_the_organization_and_their_role() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;

    let body = ok_json(
        instance
            .get(&org_path(&organization_id), Some(&token))
            .await,
    )
    .await;
    assert_eq!(
        body,
        json!({
            "id": organization_id,
            "name": "personal",
            "slug": "personal",
            "actor": { "type": "user", "id": owner.id, "role": "owner" },
        })
    );
    instance.close().await;
}

#[tokio::test]
async fn an_organizations_key_reads_it() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "company").await;
    let key = instance.create_key(&owner, &organization_id).await;
    let token = instance.key_token(&key).await;

    let body = ok_json(
        instance
            .get(&org_path(&organization_id), Some(&token))
            .await,
    )
    .await;
    assert_eq!(body["actor"], json!({ "type": "api_key", "id": key.id }));
    instance.close().await;
}

#[tokio::test]
async fn a_request_without_a_valid_token_is_unauthorized() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let path = org_path(&organization_id);
    let in_an_hour = || now_seconds() + 3600;

    let missing_sub = instance.sign_token(json!({})).await;
    let unknown_prefix = instance.sign_token(json!({ "sub": "abc_0123" })).await;
    let expired = instance
        .sign_token(json!({ "sub": owner.id, "exp": now_seconds() - 120 }))
        .await;
    let wrong_audience = instance
        .sign_token(json!({ "sub": owner.id, "aud": "elsewhere", "exp": in_an_hour() }))
        .await;
    let wrong_issuer = instance
        .sign_token(
            json!({ "sub": owner.id, "iss": "https://elsewhere.example", "exp": in_an_hour() }),
        )
        .await;
    let valid = instance.sign_token(json!({ "sub": owner.id })).await;
    let tampered = format!("{}x", valid);

    let cases = [
        ("no token", None),
        ("not a JWT", Some("not-a-token")),
        ("missing sub", Some(missing_sub.as_str())),
        ("sub with neither prefix", Some(unknown_prefix.as_str())),
        ("expired", Some(expired.as_str())),
        ("wrong audience", Some(wrong_audience.as_str())),
        ("wrong issuer", Some(wrong_issuer.as_str())),
        ("bad signature", Some(tampered.as_str())),
    ];
    for (case, token) in cases {
        let response = instance.get(&path, token).await;
        assert_eq!(response.headers()["www-authenticate"], "Bearer", "{case}");
        assert_eq!(
            refusal(response).await,
            (StatusCode::UNAUTHORIZED, "unauthorized".into()),
            "{case}"
        );
    }
    assert_eq!(
        instance.get(&path, Some(&valid)).await.status(),
        StatusCode::OK
    );
    instance.close().await;
}

#[tokio::test]
async fn an_organization_the_caller_cannot_act_in_is_not_found() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let outsider = instance.sign_up("outsider@example.com").await;
    let organization_id = instance.create_organization(&owner, "company").await;
    let other_organization_id = instance.create_organization(&outsider, "other").await;
    let outsider_token = instance.session_token(&outsider).await;
    let other_key = instance.create_key(&outsider, &other_organization_id).await;
    let other_key_token = instance.key_token(&other_key).await;
    let not_found = (StatusCode::NOT_FOUND, "not_found".to_owned());

    let cases = [
        (
            "a user who is not a member",
            org_path(&organization_id),
            &outsider_token,
        ),
        (
            "another organization's key",
            org_path(&organization_id),
            &other_key_token,
        ),
        (
            "an organization that does not exist",
            org_path("org_01m36mep00e3ca12pwd1gkjwex"),
            &outsider_token,
        ),
        (
            "a key, for an organization that does not exist",
            org_path("org_01m36mep00e3ca12pwd1gkjwex"),
            &other_key_token,
        ),
    ];
    for (case, path, token) in cases {
        assert_eq!(
            refusal(instance.get(&path, Some(token)).await).await,
            not_found,
            "{case}"
        );
    }
    instance.close().await;
}

#[tokio::test]
async fn a_disabled_expired_or_deleted_key_is_unauthorized_while_its_token_is_valid() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "company").await;
    let path = org_path(&organization_id);
    let unauthorized = (StatusCode::UNAUTHORIZED, "unauthorized".to_owned());

    let disabled = instance.create_key(&owner, &organization_id).await;
    let token = instance.key_token(&disabled).await;
    assert_eq!(
        instance.get(&path, Some(&token)).await.status(),
        StatusCode::OK
    );
    instance
        .auth_post(
            &owner,
            "/api-key/update",
            json!({ "keyId": disabled.id, "enabled": false }),
        )
        .await;
    assert_eq!(
        refusal(instance.get(&path, Some(&token)).await).await,
        unauthorized,
        "disabled"
    );

    let expired = instance.create_key(&owner, &organization_id).await;
    let token = instance.key_token(&expired).await;
    sqlx::query(
        r#"UPDATE auth.apikey SET "expiresAt" = now() - interval '1 minute' WHERE id = $1"#,
    )
    .bind(&expired.id)
    .execute(&instance.admin)
    .await
    .unwrap();
    assert_eq!(
        refusal(instance.get(&path, Some(&token)).await).await,
        unauthorized,
        "expired"
    );

    let deleted = instance.create_key(&owner, &organization_id).await;
    let token = instance.key_token(&deleted).await;
    instance
        .auth_post(&owner, "/api-key/delete", json!({ "keyId": deleted.id }))
        .await;
    assert_eq!(
        refusal(instance.get(&path, Some(&token)).await).await,
        unauthorized,
        "deleted"
    );
    instance.close().await;
}

#[tokio::test]
async fn membership_and_role_changes_apply_on_the_next_request() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let teammate = instance.sign_up("teammate@example.com").await;
    let organization_id = instance.create_organization(&owner, "company").await;
    let path = org_path(&organization_id);
    instance
        .add_member(
            &owner,
            &organization_id,
            &teammate,
            "teammate@example.com",
            "owner",
        )
        .await;
    // A key the teammate creates belongs to the organization, not to them.
    let key = instance.create_key(&teammate, &organization_id).await;
    let key_token = instance.key_token(&key).await;
    let token = instance.session_token(&teammate).await;

    let body = ok_json(instance.get(&path, Some(&token)).await).await;
    assert_eq!(body["actor"]["role"], "owner");

    let member_id: String = sqlx::query_scalar(
        r#"SELECT id FROM auth.member WHERE "organizationId" = $1 AND "userId" = $2"#,
    )
    .bind(&organization_id)
    .bind(&teammate.id)
    .fetch_one(&instance.admin)
    .await
    .unwrap();
    let body =
        json!({ "organizationId": organization_id, "memberId": member_id, "role": "member" });
    instance
        .auth_post(&owner, "/organization/update-member-role", body)
        .await;
    let body = ok_json(instance.get(&path, Some(&token)).await).await;
    assert_eq!(body["actor"]["role"], "member", "role change");

    let body =
        json!({ "organizationId": organization_id, "memberIdOrEmail": "teammate@example.com" });
    instance
        .auth_post(&owner, "/organization/remove-member", body)
        .await;
    assert_eq!(
        refusal(instance.get(&path, Some(&token)).await).await,
        (StatusCode::NOT_FOUND, "not_found".into()),
        "removed member"
    );
    assert_eq!(
        instance.get(&path, Some(&key_token)).await.status(),
        StatusCode::OK,
        "key created by the removed member"
    );
    instance.close().await;
}

#[tokio::test]
async fn tokens_verify_across_a_signing_key_rotation() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let path = org_path(&organization_id);
    let before = instance.session_token(&owner).await;
    assert_eq!(
        instance.get(&path, Some(&before)).await.status(),
        StatusCode::OK
    );

    // Better Auth signs with a new key once the current one expires, and keeps
    // publishing the old one through a grace period.
    sqlx::query(r#"UPDATE auth.jwks SET "expiresAt" = now() - interval '1 second'"#)
        .execute(&instance.admin)
        .await
        .unwrap();
    let after = instance.session_token(&owner).await;
    let kid = |token: &str| jsonwebtoken::decode_header(token).unwrap().kid;
    assert_ne!(kid(&before), kid(&after), "Auth rotated its signing key");

    assert_eq!(
        instance.get(&path, Some(&after)).await.status(),
        StatusCode::OK,
        "new key"
    );
    assert_eq!(
        instance.get(&path, Some(&before)).await.status(),
        StatusCode::OK,
        "old key"
    );
    instance.close().await;
}

#[tokio::test]
async fn migrations_run_again_without_change() {
    let instance = Instance::start().await;
    pipewhere_api::migrate(&instance.api_db).await.unwrap();
    let ledger: Option<String> =
        sqlx::query_scalar("SELECT to_regclass('pipewhere._sqlx_migrations')::text")
            .fetch_one(&instance.admin)
            .await
            .unwrap();
    assert_eq!(ledger.as_deref(), Some("pipewhere._sqlx_migrations"));
    instance.close().await;
}

fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
