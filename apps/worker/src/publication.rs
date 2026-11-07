use std::sync::Arc;

use pipewhere_api::publications::PublicationState;
use pipewhere_api::runs::{Adapter, step};
use restate_sdk::prelude::*;
use sqlx::PgPool;

/// A publication's run, keyed by its id, so Restate runs one at a time per
/// publication.
pub struct Publication<A> {
    pub db: PgPool,
    pub adapter: Arc<A>,
}

// Restate's default pauses an invocation after 70 attempts, about an hour, and
// it stays paused after the failure clears. Its publication would then never
// reach its deadline check. Repeating a step is safe, per 00003 D6, so this limit
// is one no outage reaches, at one attempt a minute.
#[object(invocation_retry_policy(max_attempts = 4294967295))]
impl<A: Adapter + Send + Sync + 'static> Publication<A> {
    /// An in-flight run survives a Worker upgrade because its journal holds only
    /// `step` entries of this type. Any other journal entry, a renamed step, or
    /// another return type needs what 00003 D6 asks for first.
    #[handler]
    async fn run(&self, ctx: ObjectContext<'_>) -> HandlerResult<()> {
        let publication_id = ctx.key();
        loop {
            let Json(state) = ctx
                .run(|| async {
                    let state = step(&self.db, &*self.adapter, publication_id).await?;
                    Ok(Json(state))
                })
                .name("step")
                .await?;
            if state != PublicationState::Preparing {
                return Ok(());
            }
        }
    }
}
