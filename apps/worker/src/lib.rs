//! Worker runs publications through Restate, per 00003 D5 and D6. The steps
//! themselves are in API's library, so both write through one application layer.

use serde_json::{Value, json};

pub mod poller;
pub mod publication;

pub type Error = Box<dyn std::error::Error + Send + Sync>;

// Restate keeps a deployment's handlers when its URI is registered again, even
// after they changed, unless the registration forces it. Forcing replaces them
// under the same deployment, so an invocation in flight resumes on the new code.
pub async fn register(admin_url: &str, worker_url: &str) -> Result<(), Error> {
    let response = reqwest::Client::new()
        .post(format!("{admin_url}/deployments"))
        .json(&json!({ "uri": worker_url, "force": true }))
        .send()
        .await
        .map_err(|error| format!("Restate's admin API at {admin_url} is unreachable: {error}"))?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Err(format!("Restate refused to register {worker_url}: {status} {body}").into());
    }
    let deployment: Value = serde_json::from_str(&body)?;
    tracing::info!(
        id = %deployment["id"].as_str().unwrap_or_default(),
        services = %deployment["services"].as_array().map_or(0, Vec::len),
        "registered {worker_url} with Restate"
    );
    Ok(())
}
