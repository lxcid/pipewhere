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

#[object]
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
