//! Installation-wide scheduler health behind organization authorization, per 00003 D10.

mod support;

use reqwest::StatusCode;
use serde_json::json;
use support::{Instance, ok_json, refusal};

fn path(org: &str) -> String {
    format!("/v1/orgs/{org}/scheduler")
}

#[tokio::test]
async fn scheduler_reports_absent_recent_and_stale_ticks_without_writing() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let org = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;
    let url = path(&org);
    assert_eq!(
        ok_json(instance.get(&url, Some(&token)).await).await,
        json!({ "last_completed_at": null, "stalled": true })
    );
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM pipewhere.scheduler_tick")
        .fetch_one(&instance.api_db)
        .await
        .unwrap();
    assert_eq!(rows, 0);

    for (age, stalled) in [(0, false), (59, false), (61, true)] {
        let at: String = sqlx::query_scalar(
            "INSERT INTO pipewhere.scheduler_tick (singleton, last_completed_at)
             VALUES (true, clock_timestamp() - $1 * interval '1 second')
             ON CONFLICT (singleton) DO UPDATE SET last_completed_at = EXCLUDED.last_completed_at
             RETURNING to_char(last_completed_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')")
            .bind(age as f64).fetch_one(&instance.api_db).await.unwrap();
        let response = ok_json(instance.get(&url, Some(&token)).await).await;
        assert_eq!(response["stalled"], stalled, "age {age}");
        let stored: chrono::DateTime<chrono::Utc> = at.parse().unwrap();
        let reported: chrono::DateTime<chrono::Utc> = response["last_completed_at"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(reported, stored);
    }
    instance.close().await;
}

#[tokio::test]
async fn members_and_keys_read_the_same_installation_tick() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let org = instance.create_organization(&owner, "company").await;
    let teammate = instance.sign_up("teammate@example.com").await;
    instance
        .add_member(&owner, &org, &teammate, "teammate@example.com", "member")
        .await;
    let other = instance.create_organization(&owner, "personal").await;
    let key = instance.create_key(&owner, &org).await;
    let tokens = [
        instance.session_token(&teammate).await,
        instance.key_token(&key).await,
    ];
    sqlx::query("INSERT INTO pipewhere.scheduler_tick (singleton, last_completed_at) VALUES (true, clock_timestamp())")
        .execute(&instance.api_db).await.unwrap();
    let expected = ok_json(
        instance
            .get(&path(&other), Some(&instance.session_token(&owner).await))
            .await,
    )
    .await;
    for token in tokens {
        assert_eq!(
            ok_json(instance.get(&path(&org), Some(&token)).await).await,
            expected
        );
    }
    instance.close().await;
}

#[tokio::test]
async fn scheduler_requires_current_membership_or_an_organizations_key() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let org = instance.create_organization(&owner, "company").await;
    let other = instance.create_organization(&owner, "personal").await;
    let outsider = instance.sign_up("outsider@example.com").await;
    let key = instance.create_key(&owner, &other).await;
    for token in [
        instance.session_token(&outsider).await,
        instance.key_token(&key).await,
    ] {
        assert_eq!(
            refusal(instance.get(&path(&org), Some(&token)).await).await,
            (StatusCode::NOT_FOUND, "not_found".to_owned())
        );
    }
    assert_eq!(
        refusal(instance.get(&path(&org), None).await).await,
        (StatusCode::UNAUTHORIZED, "unauthorized".to_owned())
    );
    let token = instance.session_token(&owner).await;
    sqlx::query(r#"DELETE FROM auth.member WHERE "organizationId" = $1"#)
        .bind(&org)
        .execute(&instance.admin)
        .await
        .unwrap();
    assert_eq!(
        refusal(instance.get(&path(&org), Some(&token)).await).await,
        (StatusCode::NOT_FOUND, "not_found".to_owned())
    );
    instance.close().await;
}

#[tokio::test]
async fn scheduler_table_allows_only_one_non_null_tick() {
    let instance = Instance::start().await;
    for singleton in [false, true] {
        let result = sqlx::query("INSERT INTO pipewhere.scheduler_tick (singleton, last_completed_at) VALUES ($1, clock_timestamp())")
            .bind(singleton).execute(&instance.api_db).await;
        assert_eq!(result.is_ok(), singleton);
    }
    assert!(sqlx::query("INSERT INTO pipewhere.scheduler_tick (singleton, last_completed_at) VALUES (true, clock_timestamp())")
        .execute(&instance.api_db).await.is_err());
    assert!(
        sqlx::query("UPDATE pipewhere.scheduler_tick SET last_completed_at = null")
            .execute(&instance.api_db)
            .await
            .is_err()
    );
    instance.close().await;
}
