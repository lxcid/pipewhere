//! How API fetches Auth's signing keys, against a stand-in key endpoint.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use axum::routing::get;
use axum::{Json, Router};
use jsonwebtoken::{EncodingKey, Header};
use pipewhere_api::tokens::{TokenError, TokenVerifier};
use serde_json::json;
use tokio::task::JoinSet;

// Lookup fails before the signature is checked, so the token's algorithm and
// signature need not be real.
fn unknown_kid_token(kid: &str) -> String {
    let header = Header {
        kid: Some(kid.to_owned()),
        ..Header::default()
    };
    jsonwebtoken::encode(&header, &json!({}), &EncodingKey::from_secret(b"unused")).unwrap()
}

/// A key endpoint with no keys that answers after `delay`, and its fetch count.
async fn key_endpoint(delay: Duration) -> (String, Arc<AtomicUsize>) {
    let fetches = Arc::new(AtomicUsize::new(0));
    let counter = fetches.clone();
    let app = Router::new().route(
        "/jwks",
        get(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            async move {
                tokio::time::sleep(delay).await;
                Json(json!({ "keys": [] }))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/jwks", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await });
    (url, fetches)
}

async fn assert_invalid(verifier: &TokenVerifier, kid: &str) {
    assert!(matches!(
        verifier.verify(&unknown_kid_token(kid)).await,
        Err(TokenError::Invalid)
    ));
}

#[tokio::test]
async fn unknown_key_ids_do_not_refetch_during_the_cooldown() {
    let (url, fetches) = key_endpoint(Duration::ZERO).await;
    let verifier = TokenVerifier::new("http://auth.test", url);

    for kid in ["a", "b", "c", "d", "e"] {
        assert_invalid(&verifier, kid).await;
    }
    assert_eq!(fetches.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn concurrent_unknown_key_ids_share_one_fetch() {
    let (url, fetches) = key_endpoint(Duration::from_millis(100)).await;
    let verifier = Arc::new(TokenVerifier::new("http://auth.test", url));

    let mut verifications = JoinSet::new();
    for n in 0..16 {
        let verifier = verifier.clone();
        verifications.spawn(async move { assert_invalid(&verifier, &format!("kid-{n}")).await });
    }
    while let Some(verification) = verifications.join_next().await {
        verification.unwrap();
    }
    assert_eq!(fetches.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn an_unknown_key_id_refetches_once_the_cooldown_ends() {
    let (url, fetches) = key_endpoint(Duration::ZERO).await;
    let verifier =
        TokenVerifier::new("http://auth.test", url).with_miss_cooldown(Duration::from_millis(200));

    assert_invalid(&verifier, "a").await;
    assert_invalid(&verifier, "b").await;
    assert_eq!(fetches.load(Ordering::SeqCst), 1);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_invalid(&verifier, "c").await;
    assert_eq!(fetches.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn requests_queued_behind_a_hanging_auth_answer_within_the_timeout() {
    // Accepts connections and never answers.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/jwks", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let mut open = Vec::new();
        while let Ok((connection, _)) = listener.accept().await {
            open.push(connection);
        }
    });
    let verifier = Arc::new(TokenVerifier::new("http://auth.test", url));

    let started = Instant::now();
    let mut verifications = JoinSet::new();
    for n in 0..3 {
        let verifier = verifier.clone();
        verifications.spawn(async move {
            verifier
                .verify(&unknown_kid_token(&format!("kid-{n}")))
                .await
        });
    }
    while let Some(verification) = verifications.join_next().await {
        assert!(matches!(
            verification.unwrap(),
            Err(TokenError::AuthUnavailable(_))
        ));
    }
    assert!(
        started.elapsed() < Duration::from_secs(6),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn an_unreachable_auth_is_reported_as_unavailable() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/jwks", listener.local_addr().unwrap());
    drop(listener);
    let verifier = TokenVerifier::new("http://auth.test", url);

    assert!(matches!(
        verifier.verify(&unknown_kid_token("a")).await,
        Err(TokenError::AuthUnavailable(_))
    ));
}
