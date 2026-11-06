//! Reading a publication's attempts, per 00002 D1, D4, and D6. Only the worker
//! records attempts, so these tests insert them directly.

mod support;

use pipewhere_api::ids::new_id;
use reqwest::{Response, StatusCode};
use serde_json::json;
use sqlx::PgPool;
use support::{Instance, ok_json, refusal};

/// A `failed` publication with its own channel and content.
async fn insert_publication(db: &PgPool, organization_id: &str) -> String {
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
             (id, organization_id, channel_id, content_id, body, state, planned_at, tolerance_seconds)
         VALUES ($1, $2, $3, $4, 'Copy', 'failed', '2026-12-01T09:00:00Z', 1800)",
    )
    .bind(&id)
    .bind(organization_id)
    .bind(&channel_id)
    .bind(&content_id)
    .execute(db)
    .await
    .unwrap();
    id
}

/// An attempt that started at `started_at`, and ended a minute later when it
/// has a result.
async fn insert_attempt(
    db: &PgPool,
    publication_id: &str,
    result: Option<&str>,
    reason: Option<&str>,
    started_at: &str,
) -> Result<String, Option<String>> {
    let id = new_id("publication_attempt");
    sqlx::query(
        "INSERT INTO pipewhere.publication_attempt
             (id, publication_id, result, reason, started_at, ended_at)
         VALUES ($1, $2, $3, $4, $5::timestamptz,
             CASE WHEN $3 IS NOT NULL THEN $5::timestamptz + interval '1 minute' END)",
    )
    .bind(&id)
    .bind(publication_id)
    .bind(result)
    .bind(reason)
    .bind(started_at)
    .execute(db)
    .await
    .map(|_| id)
    .map_err(|error| {
        error
            .into_database_error()
            .and_then(|error| error.constraint().map(str::to_owned))
    })
}

async fn list(
    instance: &Instance,
    organization_id: &str,
    token: &str,
    publication_id: &str,
) -> Response {
    instance
        .get(
            &format!("/v1/orgs/{organization_id}/publications/{publication_id}/attempts"),
            Some(token),
        )
        .await
}

#[tokio::test]
async fn the_list_shows_each_attempt_newest_first() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;
    let db = &instance.api_db;
    let publication_id = insert_publication(db, &organization_id).await;

    // Before its first run, a publication has no attempts.
    assert_eq!(
        ok_json(list(&instance, &organization_id, &token, &publication_id).await).await,
        json!({ "attempts": [] })
    );

    let late = insert_attempt(
        db,
        &publication_id,
        Some("failed"),
        Some("Missed its planned time by more than 30 minutes"),
        "2026-12-01T09:00:00Z",
    )
    .await
    .unwrap();
    let unknown = insert_attempt(
        db,
        &publication_id,
        Some("outcome_unknown"),
        Some("Mastodon did not answer within 30 seconds of the post request"),
        "2026-12-01T10:00:00Z",
    )
    .await
    .unwrap();
    let running = insert_attempt(db, &publication_id, None, None, "2026-12-01T11:00:00Z")
        .await
        .unwrap();
    // Another publication's attempt does not appear.
    let other = insert_publication(db, &organization_id).await;
    insert_attempt(db, &other, Some("published"), None, "2026-12-01T09:00:00Z")
        .await
        .unwrap();

    assert_eq!(
        ok_json(list(&instance, &organization_id, &token, &publication_id).await).await,
        json!({ "attempts": [
            {
                "id": running,
                "publication_id": publication_id,
                "result": null,
                "reason": null,
                "started_at": "2026-12-01T11:00:00Z",
                "ended_at": null,
            },
            {
                "id": unknown,
                "publication_id": publication_id,
                "result": "outcome_unknown",
                "reason": "Mastodon did not answer within 30 seconds of the post request",
                "started_at": "2026-12-01T10:00:00Z",
                "ended_at": "2026-12-01T10:01:00Z",
            },
            {
                "id": late,
                "publication_id": publication_id,
                "result": "failed",
                "reason": "Missed its planned time by more than 30 minutes",
                "started_at": "2026-12-01T09:00:00Z",
                "ended_at": "2026-12-01T09:01:00Z",
            },
        ] })
    );
    instance.close().await;
}

#[tokio::test]
async fn another_organizations_attempts_are_not_found() {
    let instance = Instance::start().await;
    let user = instance.sign_up("user@example.com").await;
    let personal = instance.create_organization(&user, "personal").await;
    let company = instance.create_organization(&user, "company").await;
    let token = instance.session_token(&user).await;
    let outsider = instance.sign_up("outsider@example.com").await;
    let outsider_token = instance.session_token(&outsider).await;
    let db = &instance.api_db;
    let attempted = insert_publication(db, &company).await;
    insert_attempt(
        db,
        &attempted,
        Some("failed"),
        Some("Mastodon refused the access token"),
        "2026-12-01T09:00:00Z",
    )
    .await
    .unwrap();
    let unattempted = insert_publication(db, &company).await;

    let not_found = (StatusCode::NOT_FOUND, "not_found".to_owned());
    for (case, organization_id, token, publication_id) in [
        (
            "another organization's publication with attempts",
            &personal,
            &token,
            attempted.as_str(),
        ),
        (
            "another organization's publication without attempts",
            &personal,
            &token,
            &unattempted,
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
            &attempted,
        ),
    ] {
        assert_eq!(
            refusal(list(&instance, organization_id, token, publication_id).await).await,
            not_found,
            "{case}"
        );
    }
    let attempts = ok_json(list(&instance, &company, &token, &attempted).await).await;
    assert_eq!(attempts["attempts"].as_array().unwrap().len(), 1);
    instance.close().await;
}

#[tokio::test]
async fn the_table_refuses_an_attempt_it_cannot_explain() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let db = &instance.api_db;
    let publication_id = insert_publication(db, &organization_id).await;
    let at = "2026-12-01T09:00:00Z";
    let constraint = |name: &str| Some(name.to_owned());

    let insert = async |id: &str, result: Option<&str>, ended_at: Option<&str>| {
        sqlx::query(
            "INSERT INTO pipewhere.publication_attempt
                 (id, publication_id, result, reason, started_at, ended_at)
             VALUES ($1, $2, $3, 'A reason', $4::timestamptz, $5::timestamptz)",
        )
        .bind(id)
        .bind(&publication_id)
        .bind(result)
        .bind(at)
        .bind(ended_at)
        .execute(db)
        .await
        .map(|_| ())
        .map_err(|error| {
            error
                .into_database_error()
                .and_then(|error| error.constraint().map(str::to_owned))
        })
    };
    assert_eq!(
        insert(&new_id("publication"), None, None)
            .await
            .err()
            .flatten(),
        constraint("publication_attempt_id_check"),
        "another table's prefix"
    );
    let id = new_id("publication_attempt");
    assert_eq!(
        insert(&id, Some("running"), Some(at)).await.err().flatten(),
        constraint("publication_attempt_result_check"),
        "a result outside the list"
    );
    assert_eq!(
        insert(&id, Some("failed"), None).await.err().flatten(),
        constraint("publication_attempt_ended_check"),
        "a result without an end"
    );
    assert_eq!(
        insert(&id, None, Some(at)).await.err().flatten(),
        constraint("publication_attempt_ended_check"),
        "an end without a result"
    );
    for result in ["failed", "outcome_unknown"] {
        assert_eq!(
            insert_attempt(db, &publication_id, Some(result), None, at)
                .await
                .err()
                .flatten(),
            constraint("publication_attempt_reason_check"),
            "{result} without a reason"
        );
    }
    for result in ["published", "cancelled"] {
        insert_attempt(db, &publication_id, Some(result), None, at)
            .await
            .unwrap_or_else(|error| panic!("{result} needs no reason: {error:?}"));
    }
    assert_eq!(
        insert_attempt(db, &new_id("publication"), None, None, at)
            .await
            .err()
            .flatten(),
        constraint("publication_attempt_publication_id_fkey"),
        "a publication that does not exist"
    );

    // Deleting a publication never deletes its attempts.
    insert_attempt(db, &publication_id, None, None, at)
        .await
        .unwrap();
    let error = sqlx::query("DELETE FROM pipewhere.publication WHERE id = $1")
        .bind(&publication_id)
        .execute(&instance.admin)
        .await
        .unwrap_err();
    assert_eq!(
        error.into_database_error().unwrap().constraint(),
        Some("publication_attempt_publication_id_fkey")
    );
    // An attempt reaches its organization through its publication, whose
    // reference to the organization never cascades, per 00001 D2.
    let references: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT conname::text, confrelid::regclass::text, confdeltype::text FROM pg_constraint
         WHERE contype = 'f' AND conrelid = 'pipewhere.publication_attempt'::regclass",
    )
    .fetch_all(&instance.admin)
    .await
    .unwrap();
    assert_eq!(
        references,
        [(
            "publication_attempt_publication_id_fkey".to_owned(),
            "pipewhere.publication".to_owned(),
            "a".to_owned()
        )]
    );
    instance.close().await;
}
