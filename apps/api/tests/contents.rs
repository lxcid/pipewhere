//! Creating, reading, and editing content, per 00002 D1, D2, and D6.

mod support;

use pipewhere_api::ids::new_id;
use reqwest::StatusCode;
use reqwest::header::CONTENT_TYPE;
use serde_json::{Value, json};
use support::{Instance, ok_json, refusal};

async fn created(response: reqwest::Response) -> Value {
    assert_eq!(response.status(), StatusCode::CREATED);
    ok_json(response).await
}

async fn stored_bodies(instance: &Instance) -> Vec<String> {
    sqlx::query_scalar("SELECT body FROM pipewhere.content ORDER BY id")
        .fetch_all(&instance.admin)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_member_creates_reads_and_edits_content() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;
    let contents = format!("/v1/orgs/{organization_id}/contents");

    let content = created(
        instance
            .post(&contents, Some(&token), json!({ "body": "First draft" }))
            .await,
    )
    .await;
    let id = content["id"].as_str().unwrap().to_owned();
    assert!(id.starts_with("cnt_"), "{id}");
    assert_eq!(
        content,
        json!({ "id": id, "organization_id": organization_id, "body": "First draft" })
    );

    let path = format!("{contents}/{id}");
    assert_eq!(
        ok_json(instance.get(&path, Some(&token)).await).await,
        content
    );

    let edited = json!({ "id": id, "organization_id": organization_id, "body": "Second draft" });
    let body = ok_json(
        instance
            .patch(&path, Some(&token), json!({ "body": "Second draft" }))
            .await,
    )
    .await;
    assert_eq!(body, edited);
    assert_eq!(
        ok_json(instance.get(&path, Some(&token)).await).await,
        edited
    );
    instance.close().await;
}

#[tokio::test]
async fn an_organizations_key_creates_and_edits_content() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "company").await;
    let key = instance.create_key(&owner, &organization_id).await;
    let token = instance.key_token(&key).await;
    let contents = format!("/v1/orgs/{organization_id}/contents");

    let content = created(
        instance
            .post(&contents, Some(&token), json!({ "body": "From a key" }))
            .await,
    )
    .await;
    let path = format!("{contents}/{}", content["id"].as_str().unwrap());
    let body = ok_json(
        instance
            .patch(&path, Some(&token), json!({ "body": "Edited by a key" }))
            .await,
    )
    .await;
    assert_eq!(body["body"], "Edited by a key");
    instance.close().await;
}

#[tokio::test]
async fn another_organizations_content_is_not_found() {
    let instance = Instance::start().await;
    let user = instance.sign_up("user@example.com").await;
    let personal = instance.create_organization(&user, "personal").await;
    let company = instance.create_organization(&user, "company").await;
    let token = instance.session_token(&user).await;
    let company_content = created(
        instance
            .post(
                &format!("/v1/orgs/{company}/contents"),
                Some(&token),
                json!({ "body": "Company writing" }),
            )
            .await,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let not_found = (StatusCode::NOT_FOUND, "not_found".to_owned());
    let edit = json!({ "body": "Overwritten" });

    let cases = [
        (
            "another organization's content",
            format!("/v1/orgs/{personal}/contents/{company_content}"),
        ),
        (
            "content that does not exist",
            format!("/v1/orgs/{personal}/contents/{}", new_id("content")),
        ),
        (
            "an id that is not a content id",
            format!("/v1/orgs/{personal}/contents/not-an-id"),
        ),
    ];
    for (case, path) in cases {
        assert_eq!(
            refusal(instance.get(&path, Some(&token)).await).await,
            not_found,
            "read {case}"
        );
        assert_eq!(
            refusal(instance.patch(&path, Some(&token), edit.clone()).await).await,
            not_found,
            "edit {case}"
        );
    }

    let outsider = instance.sign_up("outsider@example.com").await;
    let outsider_token = instance.session_token(&outsider).await;
    let contents = format!("/v1/orgs/{company}/contents");
    let path = format!("{contents}/{company_content}");
    assert_eq!(
        refusal(
            instance
                .post(&contents, Some(&outsider_token), edit.clone())
                .await
        )
        .await,
        not_found,
        "a user who is not a member creates"
    );
    assert_eq!(
        refusal(instance.get(&path, Some(&outsider_token)).await).await,
        not_found,
        "a user who is not a member reads"
    );
    assert_eq!(
        refusal(
            instance
                .patch(&path, Some(&outsider_token), edit.clone())
                .await
        )
        .await,
        not_found,
        "a user who is not a member edits"
    );

    assert_eq!(stored_bodies(&instance).await, ["Company writing"]);
    instance.close().await;
}

#[tokio::test]
async fn a_body_api_cannot_read_is_refused() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;
    let contents = format!("/v1/orgs/{organization_id}/contents");
    let content = created(
        instance
            .post(&contents, Some(&token), json!({ "body": "Kept" }))
            .await,
    )
    .await;
    let path = format!("{contents}/{}", content["id"].as_str().unwrap());

    let raw = async |method: reqwest::Method, url: &str, content_type: Option<&str>, body: &str| {
        let mut request = instance
            .http
            .request(method, format!("{}{url}", instance.api_url))
            .bearer_auth(&token)
            .body(body.to_owned());
        if let Some(content_type) = content_type {
            request = request.header(CONTENT_TYPE, content_type);
        }
        refusal(request.send().await.unwrap()).await
    };
    let json = Some("application/json");
    for (method, url) in [
        (reqwest::Method::POST, &contents),
        (reqwest::Method::PATCH, &path),
    ] {
        let cases = [
            ("not JSON", json, "{", StatusCode::BAD_REQUEST),
            (
                "no content type",
                None,
                r#"{"body":"x"}"#,
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
            ),
            (
                "no body field",
                json,
                "{}",
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (
                "a body that is not text",
                json,
                r#"{"body":1}"#,
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (
                "empty text",
                json,
                r#"{"body":""}"#,
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (
                "only whitespace",
                json,
                r#"{"body":" \n\t"}"#,
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
        ];
        for (case, content_type, body, status) in cases {
            assert_eq!(
                raw(method.clone(), url, content_type, body).await,
                (status, "invalid_body".to_owned()),
                "{method} {case}"
            );
        }
        let too_large = json!({ "body": "a".repeat(2 * 1024 * 1024) }).to_string();
        assert_eq!(
            raw(method.clone(), url, json, &too_large).await,
            (StatusCode::PAYLOAD_TOO_LARGE, "invalid_body".to_owned()),
            "{method} over 2 MB"
        );
    }

    // The caller is checked against the organization before the body is read.
    let outsider = instance.sign_up("outsider@example.com").await;
    let outsider_token = instance.session_token(&outsider).await;
    let response = instance
        .http
        .post(format!("{}{contents}", instance.api_url))
        .bearer_auth(&outsider_token)
        .body("{")
        .send()
        .await
        .unwrap();
    assert_eq!(
        refusal(response).await,
        (StatusCode::NOT_FOUND, "not_found".to_owned())
    );

    assert_eq!(stored_bodies(&instance).await, ["Kept"]);
    instance.close().await;
}

#[tokio::test]
async fn the_table_refuses_a_bad_id_or_organization() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let insert = async |id: &str, organization_id: &str| {
        sqlx::query(
            "INSERT INTO pipewhere.content (id, organization_id, body) VALUES ($1, $2, 'x')",
        )
        .bind(id)
        .bind(organization_id)
        .execute(&instance.api_db)
        .await
        .unwrap_err()
        .into_database_error()
        .unwrap()
        .constraint()
        .map(str::to_owned)
    };

    assert_eq!(
        insert(&new_id("channel"), &organization_id).await,
        Some("content_id_check".into())
    );
    assert_eq!(
        insert(&new_id("content"), "org_01m36mep00e3ca12pwd1gkjwex").await,
        Some("content_organization_id_fkey".into())
    );
    instance.close().await;
}

// 00001 D2: a foreign key to an organization never cascades.
#[tokio::test]
async fn an_organization_with_content_cannot_be_deleted() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;
    created(
        instance
            .post(
                &format!("/v1/orgs/{organization_id}/contents"),
                Some(&token),
                json!({ "body": "Kept" }),
            )
            .await,
    )
    .await;

    let error = sqlx::query("DELETE FROM auth.organization WHERE id = $1")
        .bind(&organization_id)
        .execute(&instance.admin)
        .await
        .unwrap_err();
    assert_eq!(
        error.into_database_error().unwrap().constraint(),
        Some("content_organization_id_fkey")
    );
    instance.close().await;
}
