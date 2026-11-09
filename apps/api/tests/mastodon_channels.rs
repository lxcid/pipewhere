//! Connecting and reconnecting a Mastodon account, per 00004 D2 to D5, against
//! a fake instance. The tests act as the operator's browser: they send the
//! authorization to the instance, approve it there, and hand back the code
//! as Web's redirect page does.

mod support;

use pipewhere_api::credentials::Credentials;
use reqwest::{Method, StatusCode};
use serde_json::json;
use support::mastodon::{Account, FakeMastodon, STAN};
use support::{CREDENTIALS_KEY, Instance, WEB_URL, ok_json, refusal};

struct Org {
    id: String,
    token: String,
}

async fn organization(instance: &Instance, email: &str, slug: &str) -> Org {
    let owner = instance.sign_up(email).await;
    let id = instance.create_organization(&owner, slug).await;
    let token = instance.session_token(&owner).await;
    Org { id, token }
}

async fn authorize(instance: &Instance, org: &Org, address: &str) -> reqwest::Response {
    let path = format!("/v1/orgs/{}/channels/mastodon/authorizations", org.id);
    instance
        .post(&path, Some(&org.token), json!({ "instance": address }))
        .await
}

async fn hand_back(
    instance: &Instance,
    org: &Org,
    authorization_id: &str,
    code: &str,
) -> reqwest::Response {
    let path = format!("/v1/orgs/{}/channels/mastodon", org.id);
    let body = json!({ "authorization_id": authorization_id, "code": code });
    instance.post(&path, Some(&org.token), body).await
}

/// Starts a connection, approves it as `account`, and hands back the code.
async fn connect(
    instance: &Instance,
    org: &Org,
    fake: &FakeMastodon,
    account: &Account,
) -> reqwest::Response {
    let authorization = ok_json(authorize(instance, org, &fake.host).await).await;
    let (code, state) = fake.approve(authorization["authorize_url"].as_str().unwrap(), account);
    assert_eq!(state, authorization["id"]);
    hand_back(instance, org, &state, &code).await
}

async fn stored_token(instance: &Instance, channel_id: &str) -> String {
    let sealed: Vec<u8> = sqlx::query_scalar(
        "SELECT access_token FROM pipewhere.mastodon_channel WHERE channel_id = $1",
    )
    .bind(channel_id)
    .fetch_one(&instance.admin)
    .await
    .unwrap();
    // A key read again, as after a restart, opens what the running API sealed.
    let credentials = Credentials::from_hex(CREDENTIALS_KEY).unwrap();
    credentials.open(channel_id, &sealed).unwrap()
}

#[tokio::test]
async fn connecting_an_account_makes_an_active_channel_with_its_handle() {
    let instance = Instance::start().await;
    let fake = FakeMastodon::start().await;
    let org = organization(&instance, "owner@example.com", "personal").await;

    let response = authorize(&instance, &org, &fake.host).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let authorization = ok_json(response).await;
    let id = authorization["id"].as_str().unwrap();
    assert!(id.starts_with("mauth_"), "{id}");
    let registrations = fake.registrations();
    assert_eq!(registrations.len(), 1);
    assert_eq!(registrations[0]["client_name"], "Pipewhere");
    assert_eq!(
        registrations[0]["redirect_uris"],
        format!("{WEB_URL}/connect/mastodon")
    );
    assert_eq!(
        registrations[0]["scopes"],
        "read:accounts write:statuses write:media"
    );
    let url = reqwest::Url::parse(authorization["authorize_url"].as_str().unwrap()).unwrap();
    let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(query["scope"], "read:accounts write:statuses write:media");
    assert_eq!(query["state"], id);

    let (code, state) = fake.approve(url.as_str(), &STAN);
    let response = hand_back(&instance, &org, &state, &code).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let channel = ok_json(response).await;
    let channel_id = channel["id"].as_str().unwrap().to_owned();
    assert_eq!(
        channel,
        json!({
            "id": channel_id,
            "organization_id": org.id,
            "provider": "mastodon",
            "handle": format!("@stan@{}", fake.host),
            "status": "active",
        })
    );
    let list = ok_json(
        instance
            .get(&format!("/v1/orgs/{}/channels", org.id), Some(&org.token))
            .await,
    )
    .await;
    assert_eq!(list, json!({ "channels": [channel] }));

    assert_eq!(
        stored_token(&instance, &channel_id).await,
        fake.token_for(1)
    );
    let (sealed, host): (Vec<u8>, String) =
        sqlx::query_as("SELECT client_secret, host FROM pipewhere.mastodon_instance")
            .fetch_one(&instance.admin)
            .await
            .unwrap();
    assert_eq!(host, fake.host);
    assert!(!sealed.windows(6).any(|window| window == b"secret"));
    instance.close().await;
}

#[tokio::test]
async fn connecting_an_account_again_reconnects_its_channel_and_keeps_its_history() {
    let instance = Instance::start().await;
    let fake = FakeMastodon::start().await;
    let org = organization(&instance, "owner@example.com", "personal").await;
    let first = ok_json(connect(&instance, &org, &fake, &STAN).await).await;
    let channel_id = first["id"].as_str().unwrap();
    let publication = ok_json(
        instance
            .request(
                Method::POST,
                &format!("/v1/orgs/{}/publications", org.id),
                Some(&org.token),
            )
            .header("idempotency-key", "reconnect-test-key-0000000001")
            .json(&json!({ "channel_id": channel_id, "body": "Hello" }))
            .send()
            .await
            .unwrap(),
    )
    .await;
    sqlx::query("UPDATE pipewhere.channel SET status = 'disconnected' WHERE id = $1")
        .bind(channel_id)
        .execute(&instance.admin)
        .await
        .unwrap();

    let renamed = Account {
        username: "stan_again",
        ..STAN
    };
    let response = connect(&instance, &org, &fake, &renamed).await;
    assert_eq!(response.status(), StatusCode::OK);
    let again = ok_json(response).await;
    assert_eq!(again["id"], channel_id);
    assert_eq!(again["status"], "active");
    assert_eq!(again["handle"], format!("@stan_again@{}", fake.host));
    assert_eq!(stored_token(&instance, channel_id).await, fake.token_for(2));
    let publication = ok_json(
        instance
            .get(
                &format!(
                    "/v1/orgs/{}/publications/{}",
                    org.id,
                    publication["id"].as_str().unwrap()
                ),
                Some(&org.token),
            )
            .await,
    )
    .await;
    assert_eq!(publication["channel_id"], channel_id);
    let list = ok_json(
        instance
            .get(&format!("/v1/orgs/{}/channels", org.id), Some(&org.token))
            .await,
    )
    .await;
    assert_eq!(list["channels"].as_array().unwrap().len(), 1);
    instance.close().await;
}

#[tokio::test]
async fn each_organization_that_links_an_account_gets_its_own_channel() {
    let instance = Instance::start().await;
    let fake = FakeMastodon::start().await;
    let personal = organization(&instance, "owner@example.com", "personal").await;
    let company = organization(&instance, "other@example.com", "company").await;

    let mine = ok_json(connect(&instance, &personal, &fake, &STAN).await).await;
    let response = connect(&instance, &company, &fake, &STAN).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let theirs = ok_json(response).await;
    assert_ne!(mine["id"], theirs["id"]);
    assert_eq!(theirs["organization_id"], company.id);
    let list = ok_json(
        instance
            .get(
                &format!("/v1/orgs/{}/channels", personal.id),
                Some(&personal.token),
            )
            .await,
    )
    .await;
    assert_eq!(list, json!({ "channels": [mine] }));
    assert_eq!(
        fake.registrations().len(),
        1,
        "one registration serves both organizations"
    );
    instance.close().await;
}

#[tokio::test]
async fn an_instance_registers_again_when_webs_address_changes() {
    let instance = Instance::start().await;
    let fake = FakeMastodon::start().await;
    let org = organization(&instance, "owner@example.com", "personal").await;
    ok_json(authorize(&instance, &org, &fake.host).await).await;
    ok_json(authorize(&instance, &org, &fake.host).await).await;
    assert_eq!(fake.registrations().len(), 1);

    sqlx::query("UPDATE pipewhere.mastodon_instance SET redirect_uri = 'https://old.example/connect/mastodon'")
        .execute(&instance.admin)
        .await
        .unwrap();
    let response = connect(&instance, &org, &fake, &STAN).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(fake.registrations().len(), 2);
    instance.close().await;
}

#[tokio::test]
async fn reconnecting_after_the_key_is_lost_registers_again() {
    let instance = Instance::start().await;
    let fake = FakeMastodon::start().await;
    let org = organization(&instance, "owner@example.com", "personal").await;
    let channel = ok_json(connect(&instance, &org, &fake, &STAN).await).await;
    let channel_id = channel["id"].as_str().unwrap();

    // What a lost CREDENTIALS_KEY leaves: secrets the running key cannot open.
    let lost = Credentials::from_hex(&"ab".repeat(32)).unwrap();
    sqlx::query("UPDATE pipewhere.mastodon_instance SET client_secret = $1")
        .bind(lost.seal(&fake.host, "secret-0"))
        .execute(&instance.admin)
        .await
        .unwrap();
    sqlx::query("UPDATE pipewhere.mastodon_channel SET access_token = $1")
        .bind(lost.seal(channel_id, &fake.token_for(1)))
        .execute(&instance.admin)
        .await
        .unwrap();

    let response = connect(&instance, &org, &fake, &STAN).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(ok_json(response).await["id"], channel_id);
    assert_eq!(fake.registrations().len(), 2);
    assert_eq!(stored_token(&instance, channel_id).await, fake.token_for(2));
    instance.close().await;
}

#[tokio::test]
async fn an_authorization_works_once_in_its_own_organization_within_an_hour() {
    let instance = Instance::start().await;
    let fake = FakeMastodon::start().await;
    let org = organization(&instance, "owner@example.com", "personal").await;
    let other = organization(&instance, "other@example.com", "other").await;

    let authorization = ok_json(authorize(&instance, &org, &fake.host).await).await;
    let (code, state) = fake.approve(authorization["authorize_url"].as_str().unwrap(), &STAN);
    let refused = refusal(hand_back(&instance, &other, &state, &code).await).await;
    assert_eq!(
        refused,
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "authorization_not_found".into()
        )
    );
    let response = hand_back(&instance, &org, &state, &code).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let refused = refusal(hand_back(&instance, &org, &state, &code).await).await;
    assert_eq!(
        refused,
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "authorization_not_found".into()
        )
    );

    for id in ["mauth_01h455vb4pex5vsknk084sn02q", "bad\u{0}id", ""] {
        let refused = refusal(hand_back(&instance, &org, id, &code).await).await;
        assert_eq!(
            refused,
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                "authorization_not_found".into()
            ),
            "{id:?}"
        );
    }

    let authorization = ok_json(authorize(&instance, &org, &fake.host).await).await;
    let (code, state) = fake.approve(authorization["authorize_url"].as_str().unwrap(), &STAN);
    sqlx::query(
        "UPDATE pipewhere.mastodon_authorization SET created_at = now() - interval '61 minutes'",
    )
    .execute(&instance.admin)
    .await
    .unwrap();
    let refused = refusal(hand_back(&instance, &org, &state, &code).await).await;
    assert_eq!(
        refused,
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "authorization_not_found".into()
        )
    );

    ok_json(authorize(&instance, &org, &fake.host).await).await;
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM pipewhere.mastodon_authorization")
        .fetch_one(&instance.admin)
        .await
        .unwrap();
    assert_eq!(
        left, 1,
        "starting a connection removes expired authorizations"
    );
    instance.close().await;
}

#[tokio::test]
async fn a_code_the_instance_refuses_answers_authorization_refused() {
    let instance = Instance::start().await;
    let fake = FakeMastodon::start().await;
    let org = organization(&instance, "owner@example.com", "personal").await;
    let authorization = ok_json(authorize(&instance, &org, &fake.host).await).await;
    let id = authorization["id"].as_str().unwrap();

    let refused = refusal(hand_back(&instance, &org, id, "not-a-code").await).await;
    assert_eq!(
        refused,
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "authorization_refused".into()
        )
    );
    let channels: i64 = sqlx::query_scalar("SELECT count(*) FROM pipewhere.channel")
        .fetch_one(&instance.admin)
        .await
        .unwrap();
    assert_eq!(channels, 0);
    instance.close().await;
}

#[tokio::test]
async fn an_instance_that_cannot_register_pipewhere_answers_instance_unavailable() {
    let instance = Instance::start().await;
    let org = organization(&instance, "owner@example.com", "personal").await;

    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = closed.local_addr().unwrap().to_string();
    drop(closed);
    let refused = refusal(authorize(&instance, &org, &address).await).await;
    assert_eq!(
        refused,
        (StatusCode::BAD_GATEWAY, "instance_unavailable".into())
    );

    let fake = FakeMastodon::start().await;
    fake.refuse_registration();
    let refused = refusal(authorize(&instance, &org, &fake.host).await).await;
    assert_eq!(
        refused,
        (StatusCode::BAD_GATEWAY, "instance_unavailable".into())
    );
    instance.close().await;
}

#[tokio::test]
async fn a_request_that_names_no_instance_or_code_answers_invalid_body() {
    let instance = Instance::start().await;
    let fake = FakeMastodon::start().await;
    let org = organization(&instance, "owner@example.com", "personal").await;

    for address in ["", "@stan@kopiti.am", "kopiti.am/@stan", "http://kopiti.am"] {
        let refused = refusal(authorize(&instance, &org, address).await).await;
        assert_eq!(
            refused,
            (StatusCode::UNPROCESSABLE_ENTITY, "invalid_body".into()),
            "{address:?}"
        );
    }
    let path = format!("/v1/orgs/{}/channels/mastodon/authorizations", org.id);
    let body = json!({ "instance": fake.host, "scopes": "read" });
    let refused = refusal(instance.post(&path, Some(&org.token), body).await).await;
    assert_eq!(
        refused,
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_body".into())
    );

    let authorization = ok_json(authorize(&instance, &org, &fake.host).await).await;
    let id = authorization["id"].as_str().unwrap();
    let refused = refusal(hand_back(&instance, &org, id, "  ").await).await;
    assert_eq!(
        refused,
        (StatusCode::UNPROCESSABLE_ENTITY, "invalid_body".into())
    );
    let (code, state) = fake.approve(authorization["authorize_url"].as_str().unwrap(), &STAN);
    let response = hand_back(&instance, &org, &state, &code).await;
    assert_eq!(
        response.status(),
        StatusCode::CREATED,
        "a refused body leaves the authorization usable"
    );
    instance.close().await;
}

#[tokio::test]
async fn an_outsider_cannot_start_or_finish_a_connection() {
    let instance = Instance::start().await;
    let fake = FakeMastodon::start().await;
    let org = organization(&instance, "owner@example.com", "personal").await;
    let outsider = organization(&instance, "outsider@example.com", "outsider").await;
    let intruding = Org {
        id: org.id.clone(),
        token: outsider.token.clone(),
    };
    let refused = refusal(authorize(&instance, &intruding, &fake.host).await).await;
    assert_eq!(refused, (StatusCode::NOT_FOUND, "not_found".into()));
    let refused = refusal(hand_back(&instance, &intruding, "mauth_x", "code").await).await;
    assert_eq!(refused, (StatusCode::NOT_FOUND, "not_found".into()));
    assert!(fake.registrations().is_empty());
    instance.close().await;
}

#[tokio::test]
async fn api_lets_webs_pages_call_it_from_the_browser() {
    let instance = Instance::start().await;
    let preflight = |origin: &'static str| {
        instance
            .request(Method::OPTIONS, "/v1/orgs/org_x/channels", None)
            .header("origin", origin)
            .header("access-control-request-method", "POST")
            .header(
                "access-control-request-headers",
                "authorization, content-type, idempotency-key",
            )
            .send()
    };
    let response = preflight(WEB_URL).await.unwrap();
    assert!(response.status().is_success(), "{}", response.status());
    let headers = response.headers();
    assert_eq!(headers["access-control-allow-origin"], WEB_URL);
    let allowed = headers["access-control-allow-headers"].to_str().unwrap();
    for header in ["authorization", "content-type", "idempotency-key"] {
        assert!(allowed.contains(header), "{allowed}");
    }

    // The answer names Web's origin whoever asks, so a browser on another
    // origin refuses to send the request.
    let response = preflight("http://evil.example").await.unwrap();
    assert_eq!(response.headers()["access-control-allow-origin"], WEB_URL);
    instance.close().await;
}
