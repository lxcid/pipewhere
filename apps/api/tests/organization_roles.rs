//! Only an owner or admin connects a channel or changes the organization's
//! settings, per 00012 D2 and D3.

mod support;

use reqwest::{Method, Response, StatusCode};
use serde_json::{Value, json};
use support::{Instance, User, ok_json, refusal};

struct Action {
    method: Method,
    path: &'static str,
    body: Value,
    /// What a caller allowed to act gets back. The bodies are chosen to fail
    /// after the role check, so no instance needs to answer.
    allowed: (StatusCode, &'static str),
}

fn actions() -> Vec<Action> {
    vec![
        Action {
            method: Method::POST,
            path: "/channels/mastodon/authorizations",
            body: json!({ "instance": "not an instance" }),
            allowed: (StatusCode::UNPROCESSABLE_ENTITY, "invalid_body"),
        },
        Action {
            method: Method::POST,
            path: "/channels/mastodon",
            body: json!({ "authorization_id": "mastodon_authorization_none", "code": "code" }),
            allowed: (StatusCode::UNPROCESSABLE_ENTITY, "authorization_not_found"),
        },
        Action {
            method: Method::PATCH,
            path: "/settings",
            body: json!({ "tolerance_seconds": 60 }),
            allowed: (StatusCode::UNPROCESSABLE_ENTITY, "invalid_body"),
        },
    ]
}

async fn send(
    instance: &Instance,
    organization_id: &str,
    token: &str,
    action: &Action,
) -> Response {
    instance
        .request(
            action.method.clone(),
            &format!("/v1/orgs/{organization_id}{}", action.path),
            Some(token),
        )
        .json(&action.body)
        .send()
        .await
        .unwrap()
}

async fn member_id(instance: &Instance, organization_id: &str, user: &User) -> String {
    sqlx::query_scalar(
        r#"SELECT id FROM auth.member WHERE "organizationId" = $1 AND "userId" = $2"#,
    )
    .bind(organization_id)
    .bind(&user.id)
    .fetch_one(&instance.admin)
    .await
    .unwrap()
}

#[tokio::test]
async fn only_an_owner_or_admin_connects_a_channel_or_changes_settings() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "team").await;
    let mut tokens = vec![("owner", instance.session_token(&owner).await)];
    for role in ["admin", "member"] {
        let email = format!("{role}@example.com");
        let user = instance.sign_up(&email).await;
        instance
            .add_member(&owner, &organization_id, &user, &email, role)
            .await;
        tokens.push((role, instance.session_token(&user).await));
    }
    let key = instance.create_key(&owner, &organization_id).await;
    tokens.push(("key", instance.key_token(&key).await));

    for action in actions() {
        for (caller, token) in &tokens {
            let expected = match *caller {
                "owner" | "admin" => (action.allowed.0, action.allowed.1.to_owned()),
                _ => (StatusCode::FORBIDDEN, "forbidden".to_owned()),
            };
            assert_eq!(
                refusal(send(&instance, &organization_id, token, &action).await).await,
                expected,
                "{caller} {} {}",
                action.method,
                action.path
            );
        }
    }
    instance.close().await;
}

#[tokio::test]
async fn a_refused_member_changes_nothing() {
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
    let token = instance.session_token(&member).await;
    let path = format!("/v1/orgs/{organization_id}/settings");

    let refused = instance
        .patch(&path, Some(&token), json!({ "tolerance_seconds": 600 }))
        .await;
    assert_eq!(
        refusal(refused).await,
        (StatusCode::FORBIDDEN, "forbidden".into())
    );
    let settings = ok_json(instance.get(&path, Some(&token)).await).await;
    assert_eq!(settings["tolerance_seconds"], 1800);
    instance.close().await;
}

// 00001 D10: a role is read on each request, so a token issued under the old
// role carries the new one.
#[tokio::test]
async fn a_role_change_applies_on_the_next_request() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "team").await;
    let teammate = instance.sign_up("teammate@example.com").await;
    instance
        .add_member(
            &owner,
            &organization_id,
            &teammate,
            "teammate@example.com",
            "member",
        )
        .await;
    let token = instance.session_token(&teammate).await;
    let teammate_member = member_id(&instance, &organization_id, &teammate).await;
    let path = format!("/v1/orgs/{organization_id}/settings");
    let change = || instance.patch(&path, Some(&token), json!({ "tolerance_seconds": 600 }));

    assert_eq!(
        change().await.status(),
        StatusCode::FORBIDDEN,
        "as a member"
    );

    for (role, expected) in [("admin", StatusCode::OK), ("member", StatusCode::FORBIDDEN)] {
        let body =
            json!({ "organizationId": organization_id, "memberId": teammate_member, "role": role });
        instance
            .auth_post(&owner, "/organization/update-member-role", body)
            .await;
        assert_eq!(change().await.status(), expected, "once {role}");
    }
    instance.close().await;
}
