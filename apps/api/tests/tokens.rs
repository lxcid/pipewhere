//! How API fetches Auth's signing keys, against a stand-in key endpoint.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::routing::get;
use axum::{Json, Router};
use pipewhere_api::tokens::{TokenError, TokenVerifier};
use serde_json::json;

// A token whose header names an unknown key. Lookup fails before the
// signature is checked, so the signature need not be real.
const UNKNOWN_KID_TOKEN: &str = "eyJhbGciOiJFZERTQSIsImtpZCI6Im5vcGUifQ.e30.c2ln";

async fn key_endpoint() -> (String, Arc<AtomicUsize>) {
    let fetches = Arc::new(AtomicUsize::new(0));
    let counter = fetches.clone();
    let app = Router::new().route(
        "/jwks",
        get(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            async { Json(json!({ "keys": [] })) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/jwks", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await });
    (url, fetches)
}

#[tokio::test]
async fn unknown_key_ids_do_not_refetch_during_the_cooldown() {
    let (url, fetches) = key_endpoint().await;
    let verifier = TokenVerifier::new("http://auth.test", url);

    for _ in 0..5 {
        assert!(matches!(
            verifier.verify(UNKNOWN_KID_TOKEN).await,
            Err(TokenError::Invalid)
        ));
    }
    assert_eq!(fetches.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn an_unreachable_auth_is_reported_as_unavailable() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/jwks", listener.local_addr().unwrap());
    drop(listener);
    let verifier = TokenVerifier::new("http://auth.test", url);

    assert!(matches!(
        verifier.verify(UNKNOWN_KID_TOKEN).await,
        Err(TokenError::AuthUnavailable(_))
    ));
}
