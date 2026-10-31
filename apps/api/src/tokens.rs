use std::collections::HashMap;
use std::time::{Duration, Instant};

use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use tokio::sync::RwLock;

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
        }
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
        {
            let keys = self.keys.read().await;
            if let Some(key) = keys.by_id.get(kid) {
                return Ok(Some(key.clone()));
            }
            if keys
                .last_miss
                .is_some_and(|at| at.elapsed() < MISS_COOLDOWN)
            {
                return Ok(None);
            }
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
                let kid = jwk.common.key_id.clone()?;
                DecodingKey::from_jwk(jwk).ok().map(|key| (kid, key))
            })
            .collect())
    }
}
