//! An organization's settings, per 00003 D1 and D11.

mod support;

use reqwest::StatusCode;
use reqwest::header::CONTENT_TYPE;
use serde_json::json;
use support::{Instance, ok_json, refusal};

fn settings_path(organization_id: &str) -> String {
    format!("/v1/orgs/{organization_id}/settings")
}

async fn stored_rows(instance: &Instance) -> Vec<(String, i32)> {
    sqlx::query_as(
        "SELECT organization_id, tolerance_seconds FROM pipewhere.organization_setting
         ORDER BY organization_id",
    )
    .fetch_all(&instance.admin)
    .await
    .unwrap()
}

#[tokio::test]
async fn a_member_reads_and_changes_the_default_tolerance() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;
    let path = settings_path(&organization_id);

    // No row means 30 minutes, and reading does not write one.
    assert_eq!(
        ok_json(instance.get(&path, Some(&token)).await).await,
        json!({ "tolerance_seconds": 1800 })
    );
    assert_eq!(stored_rows(&instance).await, []);

    for tolerance in [600, 300, 86_400] {
        let expected = json!({ "tolerance_seconds": tolerance });
        assert_eq!(
            ok_json(instance.patch(&path, Some(&token), expected.clone()).await).await,
            expected
        );
        assert_eq!(
            ok_json(instance.get(&path, Some(&token)).await).await,
            expected
        );
        assert_eq!(
            stored_rows(&instance).await,
            [(organization_id.clone(), tolerance)]
        );
    }
    instance.close().await;
}

// 00003 D11: changing the default is open to every member and key.
#[tokio::test]
async fn any_member_or_key_changes_the_default_tolerance() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "company").await;
    let path = settings_path(&organization_id);

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
    let key = instance.create_key(&owner, &organization_id).await;
    let callers = [
        (instance.session_token(&teammate).await, 600),
        (instance.key_token(&key).await, 900),
    ];
    for (token, tolerance) in callers {
        let expected = json!({ "tolerance_seconds": tolerance });
        assert_eq!(
            ok_json(instance.patch(&path, Some(&token), expected.clone()).await).await,
            expected
        );
        assert_eq!(
            ok_json(instance.get(&path, Some(&token)).await).await,
            expected
        );
    }
    instance.close().await;
}

#[tokio::test]
async fn another_organizations_settings_are_not_found() {
    let instance = Instance::start().await;
    let user = instance.sign_up("user@example.com").await;
    let personal = instance.create_organization(&user, "personal").await;
    let company = instance.create_organization(&user, "company").await;
    let token = instance.session_token(&user).await;
    let company_path = settings_path(&company);
    let edit = json!({ "tolerance_seconds": 600 });

    ok_json(
        instance
            .patch(&company_path, Some(&token), edit.clone())
            .await,
    )
    .await;
    assert_eq!(
        ok_json(instance.get(&settings_path(&personal), Some(&token)).await).await,
        json!({ "tolerance_seconds": 1800 }),
        "one organization's default does not reach another"
    );

    let outsider = instance.sign_up("outsider@example.com").await;
    let outsider_token = instance.session_token(&outsider).await;
    let key = instance.create_key(&user, &personal).await;
    let key_token = instance.key_token(&key).await;
    let not_found = (StatusCode::NOT_FOUND, "not_found".to_owned());
    for (case, caller) in [
        ("a user who is not a member", &outsider_token),
        ("another organization's key", &key_token),
    ] {
        assert_eq!(
            refusal(instance.get(&company_path, Some(caller)).await).await,
            not_found,
            "{case} reads"
        );
        assert_eq!(
            refusal(
                instance
                    .patch(
                        &company_path,
                        Some(caller),
                        json!({ "tolerance_seconds": 900 })
                    )
                    .await
            )
            .await,
            not_found,
            "{case} changes"
        );
    }

    let unauthorized = (StatusCode::UNAUTHORIZED, "unauthorized".to_owned());
    assert_eq!(
        refusal(instance.get(&company_path, None).await).await,
        unauthorized
    );
    assert_eq!(
        refusal(instance.patch(&company_path, None, edit).await).await,
        unauthorized
    );

    // The caller is checked against the organization before the body is read.
    let response = instance
        .http
        .patch(format!("{}{company_path}", instance.api_url))
        .bearer_auth(&outsider_token)
        .body("{")
        .send()
        .await
        .unwrap();
    assert_eq!(refusal(response).await, not_found);

    assert_eq!(stored_rows(&instance).await, [(company, 600)]);
    instance.close().await;
}

#[tokio::test]
async fn a_body_api_cannot_read_is_refused() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;
    let path = settings_path(&organization_id);
    ok_json(
        instance
            .patch(&path, Some(&token), json!({ "tolerance_seconds": 600 }))
            .await,
    )
    .await;

    let raw = async |content_type: Option<&str>, body: String| {
        let mut request = instance
            .http
            .patch(format!("{}{path}", instance.api_url))
            .bearer_auth(&token)
            .body(body);
        if let Some(content_type) = content_type {
            request = request.header(CONTENT_TYPE, content_type);
        }
        refusal(request.send().await.unwrap()).await
    };
    let json = Some("application/json");
    let unprocessable = StatusCode::UNPROCESSABLE_ENTITY;
    let too_large = json!({ "tolerance_seconds": 600, "x": "a".repeat(2 * 1024 * 1024) });
    let cases = [
        ("not JSON", json, "{".to_owned(), StatusCode::BAD_REQUEST),
        (
            "no content type",
            None,
            r#"{"tolerance_seconds":900}"#.to_owned(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (
            "over 2 MB",
            json,
            too_large.to_string(),
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        ("no tolerance", json, "{}".to_owned(), unprocessable),
        (
            "a null tolerance",
            json,
            r#"{"tolerance_seconds":null}"#.to_owned(),
            unprocessable,
        ),
        (
            "a field the edit does not define",
            json,
            r#"{"tolerance_seconds":900,"name":"x"}"#.to_owned(),
            unprocessable,
        ),
        (
            "text",
            json,
            r#"{"tolerance_seconds":"900"}"#.to_owned(),
            unprocessable,
        ),
        (
            "a fraction",
            json,
            r#"{"tolerance_seconds":900.5}"#.to_owned(),
            unprocessable,
        ),
        (
            "too large for the column",
            json,
            r#"{"tolerance_seconds":4294967296}"#.to_owned(),
            unprocessable,
        ),
        (
            "under 5 minutes",
            json,
            r#"{"tolerance_seconds":299}"#.to_owned(),
            unprocessable,
        ),
        (
            "over 24 hours",
            json,
            r#"{"tolerance_seconds":86401}"#.to_owned(),
            unprocessable,
        ),
        (
            "zero",
            json,
            r#"{"tolerance_seconds":0}"#.to_owned(),
            unprocessable,
        ),
        (
            "negative",
            json,
            r#"{"tolerance_seconds":-1800}"#.to_owned(),
            unprocessable,
        ),
    ];
    for (case, content_type, body, status) in cases {
        assert_eq!(
            raw(content_type, body).await,
            (status, "invalid_body".to_owned()),
            "{case}"
        );
    }

    assert_eq!(stored_rows(&instance).await, [(organization_id, 600)]);
    instance.close().await;
}

#[tokio::test]
async fn the_table_refuses_bad_rows() {
    let instance = Instance::start().await;
    let owner = instance.sign_up("owner@example.com").await;
    let organization_id = instance.create_organization(&owner, "personal").await;
    let token = instance.session_token(&owner).await;
    let insert = async |organization_id: &str, tolerance: i32| {
        sqlx::query(
            "INSERT INTO pipewhere.organization_setting (organization_id, tolerance_seconds)
             VALUES ($1, $2)",
        )
        .bind(organization_id)
        .bind(tolerance)
        .execute(&instance.api_db)
        .await
        .unwrap_err()
        .into_database_error()
        .unwrap()
        .constraint()
        .map(str::to_owned)
    };

    // 00003 D1: from 5 minutes to 24 hours.
    for tolerance in [299, 86_401] {
        assert_eq!(
            insert(&organization_id, tolerance).await,
            Some("organization_setting_tolerance_seconds_check".into())
        );
    }
    assert_eq!(
        insert("org_01m36mep00e3ca12pwd1gkjwex", 1800).await,
        Some("organization_setting_organization_id_fkey".into())
    );
    ok_json(
        instance
            .patch(
                &settings_path(&organization_id),
                Some(&token),
                json!({ "tolerance_seconds": 600 }),
            )
            .await,
    )
    .await;
    assert_eq!(
        insert(&organization_id, 1800).await,
        Some("organization_setting_pkey".into())
    );

    // 00001 D2: a foreign key to an organization never cascades.
    let error = sqlx::query("DELETE FROM auth.organization WHERE id = $1")
        .bind(&organization_id)
        .execute(&instance.admin)
        .await
        .unwrap_err();
    assert_eq!(
        error.into_database_error().unwrap().constraint(),
        Some("organization_setting_organization_id_fkey")
    );
    instance.close().await;
}
