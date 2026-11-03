//! Reading channels, per 00002 D6 and D7. Connecting a channel belongs to
//! 00004, so these tests insert channel rows directly.

mod support;

use pipewhere_api::ids::new_id;
use reqwest::StatusCode;
use serde_json::json;
use sqlx::PgPool;
use support::{Instance, ok_json, refusal};

async fn insert_channel(db: &PgPool, organization_id: &str, provider: &str) -> String {
    let id = new_id("channel");
    sqlx::query(
        "INSERT INTO pipewhere.channel (id, organization_id, provider) VALUES ($1, $2, $3)",
    )
    .bind(&id)
    .bind(organization_id)
    .bind(provider)
    .execute(db)
    .await
    .unwrap();
    id
}

#[tokio::test]
async fn a_member_lists_and_reads_the_organizations_channels() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;
    let channels = format!("/v1/orgs/{organization_id}/channels");

    let body = ok_json(instance.get(&channels, Some(&token)).await).await;
    assert_eq!(body, json!({ "channels": [] }));

    let mastodon = insert_channel(&instance.api_db, &organization_id, "mastodon").await;
    let threads = insert_channel(&instance.api_db, &organization_id, "threads").await;
    let body = ok_json(instance.get(&channels, Some(&token)).await).await;
    assert_eq!(
        body,
        json!({ "channels": [
            { "id": mastodon, "organization_id": organization_id, "provider": "mastodon" },
            { "id": threads, "organization_id": organization_id, "provider": "threads" },
        ] })
    );

    let body = ok_json(
        instance
            .get(&format!("{channels}/{threads}"), Some(&token))
            .await,
    )
    .await;
    assert_eq!(
        body,
        json!({ "id": threads, "organization_id": organization_id, "provider": "threads" })
    );
    instance.close().await;
}

#[tokio::test]
async fn an_organizations_key_reads_its_channels() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "company").await;
    let key = instance.create_key(&owner, &organization_id).await;
    let token = instance.key_token(&key).await;
    let channel = insert_channel(&instance.api_db, &organization_id, "mastodon").await;

    let path = format!("/v1/orgs/{organization_id}/channels/{channel}");
    let body = ok_json(instance.get(&path, Some(&token)).await).await;
    assert_eq!(body["id"], channel);
    instance.close().await;
}

#[tokio::test]
async fn another_organizations_channel_is_not_found() {
    let instance = Instance::start().await;
    let user = instance.sign_up("user@example.com").await;
    let personal = instance.create_organization(&user, "personal").await;
    let company = instance.create_organization(&user, "company").await;
    let token = instance.session_token(&user).await;
    let company_channel = insert_channel(&instance.api_db, &company, "mastodon").await;
    let not_found = (StatusCode::NOT_FOUND, "not_found".to_owned());

    let body = ok_json(
        instance
            .get(&format!("/v1/orgs/{personal}/channels"), Some(&token))
            .await,
    )
    .await;
    assert_eq!(body, json!({ "channels": [] }), "personal lists none");

    let cases = [
        (
            "another organization's channel",
            format!("/v1/orgs/{personal}/channels/{company_channel}"),
        ),
        (
            "a channel that does not exist",
            format!("/v1/orgs/{personal}/channels/{}", new_id("channel")),
        ),
        (
            "an id that is not a channel id",
            format!("/v1/orgs/{personal}/channels/not-an-id"),
        ),
    ];
    for (case, path) in cases {
        assert_eq!(
            refusal(instance.get(&path, Some(&token)).await).await,
            not_found,
            "{case}"
        );
    }

    let outsider = instance.sign_up("outsider@example.com").await;
    let outsider_token = instance.session_token(&outsider).await;
    for path in [
        format!("/v1/orgs/{company}/channels"),
        format!("/v1/orgs/{company}/channels/{company_channel}"),
    ] {
        assert_eq!(
            refusal(instance.get(&path, Some(&outsider_token)).await).await,
            not_found,
            "a user who is not a member: {path}"
        );
    }
    instance.close().await;
}

#[tokio::test]
async fn the_table_refuses_a_bad_id_provider_or_organization() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let insert = async |id: &str, organization_id: &str, provider: &str| {
        sqlx::query(
            "INSERT INTO pipewhere.channel (id, organization_id, provider) VALUES ($1, $2, $3)",
        )
        .bind(id)
        .bind(organization_id)
        .bind(provider)
        .execute(&instance.api_db)
        .await
        .unwrap_err()
        .into_database_error()
        .unwrap()
        .constraint()
        .map(str::to_owned)
    };

    assert_eq!(
        insert(&new_id("content"), &organization_id, "mastodon").await,
        Some("channel_id_check".into())
    );
    assert_eq!(
        insert(&new_id("channel"), &organization_id, "bluesky").await,
        Some("channel_provider_check".into())
    );
    assert_eq!(
        insert(
            &new_id("channel"),
            "org_01m36mep00e3ca12pwd1gkjwex",
            "mastodon"
        )
        .await,
        Some("channel_organization_id_fkey".into())
    );
    instance.close().await;
}

// 00001 D2: a foreign key to an organization never cascades.
#[tokio::test]
async fn an_organization_with_channels_cannot_be_deleted() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    insert_channel(&instance.api_db, &organization_id, "mastodon").await;

    let error = sqlx::query("DELETE FROM auth.organization WHERE id = $1")
        .bind(&organization_id)
        .execute(&instance.admin)
        .await
        .unwrap_err();
    assert_eq!(
        error.into_database_error().unwrap().constraint(),
        Some("channel_organization_id_fkey")
    );
    instance.close().await;
}
