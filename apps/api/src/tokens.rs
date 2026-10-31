use std::collections::HashMap;
use std::time::{Duration, Instant};

use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use tokio::sync::{Mutex, RwLock};

pub const AUDIENCE: &str = "pipewhere-api";

// A token naming an unknown key makes API refetch Auth's keys, so that a key
// Auth rotated in verifies at once. After a refetch that still lacks the key,
// further refetches wait this long, so forged key ids cannot turn every
// request into a call to Auth.
const MISS_COOLDOWN: Duration = Duration::from_secs(10);

#[derive(Debug, Deserialize)]
pub struct Claims {
    pub sub: String,
}

#[derive(Debug)]
pub enum TokenError {
    Invalid,
    AuthUnavailable(String),
}

pub struct TokenVerifier {
    jwks_url: String,
    validation: Validation,
    http: reqwest::Client,
    keys: RwLock<Keys>,
    // Held across a fetch, so requests that miss together share one fetch
    // instead of each calling Auth.
    refresh: Mutex<()>,
    miss_cooldown: Duration,
}

#[derive(Default)]
struct Keys {
    by_id: HashMap<String, DecodingKey>,
    last_miss: Option<Instant>,
}

impl TokenVerifier {
    pub fn new(issuer: &str, jwks_url: String) -> Self {
        // reqwest is built without a TLS provider, so the image does not
        // compile aws-lc; it uses ring, which sqlx already links. Installing
        // fails only when a provider is already installed.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut validation = Validation::new(Algorithm::EdDSA);
        validation.set_issuer(&[issuer]);
        validation.set_audience(&[AUDIENCE]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("an HTTP client with a timeout builds");
        Self {
            jwks_url,
            validation,
            http,
            keys: RwLock::default(),
            refresh: Mutex::default(),
            miss_cooldown: MISS_COOLDOWN,
        }
    }

    pub fn with_miss_cooldown(mut self, cooldown: Duration) -> Self {
        self.miss_cooldown = cooldown;
        self
    }

    pub async fn verify(&self, token: &str) -> Result<Claims, TokenError> {
        let header = jsonwebtoken::decode_header(token).map_err(|_| TokenError::Invalid)?;
        let kid = header.kid.ok_or(TokenError::Invalid)?;
        let key = self.key(&kid).await?.ok_or(TokenError::Invalid)?;
        jsonwebtoken::decode::<Claims>(token, &key, &self.validation)
            .map(|data| data.claims)
            .map_err(|_| TokenError::Invalid)
    }

    async fn key(&self, kid: &str) -> Result<Option<DecodingKey>, TokenError> {
        if let Some(cached) = self.cached(kid).await {
            return Ok(cached);
        }
        let _refresh = self.refresh.lock().await;
        if let Some(cached) = self.cached(kid).await {
            return Ok(cached);
        }
        let by_id = self.fetch().await?;
        let mut keys = self.keys.write().await;
        keys.by_id = by_id;
        let key = keys.by_id.get(kid).cloned();
        if key.is_none() {
            keys.last_miss = Some(Instant::now());
        }
        Ok(key)
    }

    /// The key, `Some(None)` when a refetch is cooling down, or `None` when
    /// API should fetch.
    async fn cached(&self, kid: &str) -> Option<Option<DecodingKey>> {
        let keys = self.keys.read().await;
        if let Some(key) = keys.by_id.get(kid) {
            return Some(Some(key.clone()));
        }
        keys.last_miss
            .is_some_and(|at| at.elapsed() < self.miss_cooldown)
            .then_some(None)
    }

    async fn fetch(&self) -> Result<HashMap<String, DecodingKey>, TokenError> {
        let unavailable = |error: reqwest::Error| TokenError::AuthUnavailable(error.to_string());
        let set: JwkSet = self
            .http
            .get(&self.jwks_url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(unavailable)?
            .json()
            .await
            .map_err(unavailable)?;
        Ok(set
            .keys
            .iter()
            .filter_map(|jwk| {
                // A key API cannot use makes every token it signed unauthorized,
                // so the log is the operator's only clue.
                let Some(kid) = jwk.common.key_id.clone() else {
                    tracing::warn!("ignoring a signing key from Auth without a key id");
                    return None;
                };
                match DecodingKey::from_jwk(jwk) {
                    Ok(key) => Some((kid, key)),
                    Err(error) => {
                        tracing::warn!(%kid, %error, "ignoring a signing key from Auth");
                        None
                    }
                }
            })
            .collect())
    }
}
