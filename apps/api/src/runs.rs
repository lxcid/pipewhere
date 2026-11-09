//! A publication's run, per 00003 D6, D7, and D8: the steps Worker takes through
//! Restate, here so that it writes through the same application layer as API.

use std::future::Future;

use sqlx::{PgConnection, PgPool};

use crate::channels::Provider;
use crate::ids::new_id;
use crate::publications::{AttemptResult, PublicationState, deadline};

const MISSED_ITS_TIME: &str =
    "Missed its time: its provider commit had not started by its deadline.";
const STOPPED_DURING_COMMIT: &str = "Pipewhere stopped while the commit may have started. A post may exist, so check the provider before resolving this publication.";

/// What a provider is asked to post.
#[derive(sqlx::FromRow)]
pub struct Commit {
    pub provider: Provider,
    pub body: String,
    pub publication_id: String,
    pub channel_id: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CommitResult {
    Published {
        remote_id: String,
        remote_url: Option<String>,
    },
    /// Known to have done nothing, and should not run again.
    Refused { reason: String },
    /// A post may exist.
    Ambiguous { reason: String },
}

pub trait Adapter: Sync {
    fn commit(&self, commit: &Commit) -> impl Future<Output = CommitResult> + Send;
}

/// What runs until 00004 adds a provider's adapter, per 00003 D8.
pub struct NoAdapter;

impl Adapter for NoAdapter {
    async fn commit(&self, commit: &Commit) -> CommitResult {
        let provider = match commit.provider {
            Provider::Mastodon => "Mastodon",
            Provider::Threads => "Threads",
        };
        CommitResult::Refused {
            reason: format!("Pipewhere cannot publish to {provider} yet."),
        }
    }
}

/// Takes the next step of a publication's run. The run takes another step only
/// when this answers `preparing`.
pub async fn step(
    db: &PgPool,
    adapter: &impl Adapter,
    publication_id: &str,
) -> Result<PublicationState, sqlx::Error> {
    match read_state(db, publication_id).await? {
        PublicationState::Scheduled => claim(db, publication_id).await,
        PublicationState::Preparing => commit(db, adapter, publication_id).await,
        state => settle(db, publication_id, state).await,
    }
}

async fn read_state(db: &PgPool, publication_id: &str) -> Result<PublicationState, sqlx::Error> {
    sqlx::query_scalar("SELECT state FROM pipewhere.publication WHERE id = $1")
        .bind(publication_id)
        .fetch_one(db)
        .await
}

// A run can outlive the time it was sent for, when the operator moves the
// planned time later or a retry sets a later one, so the claim checks that time.
//
// A claim that changes no row lost to another run, a cancel, or such an edit,
// so it answers the state it found, which stops the run.
async fn claim(db: &PgPool, publication_id: &str) -> Result<PublicationState, sqlx::Error> {
    let mut tx = db.begin().await?;
    let claimed: Option<PublicationState> = sqlx::query_scalar(concat!(
        "UPDATE pipewhere.publication
         SET state = CASE WHEN now() <= ",
        deadline!(),
        " THEN 'preparing' ELSE 'failed' END
         WHERE id = $1 AND state = 'scheduled' AND planned_at <= now()
         RETURNING state"
    ))
    .bind(publication_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(state) = claimed else {
        return Ok(PublicationState::Scheduled);
    };
    let (result, reason) = match state {
        PublicationState::Failed => (Some(AttemptResult::Failed), Some(MISSED_ITS_TIME)),
        _ => (None, None),
    };
    sqlx::query(
        "INSERT INTO pipewhere.publication_attempt
             (id, publication_id, result, reason, started_at, ended_at)
         VALUES ($1, $2, $3, $4, now(), CASE WHEN $3 IS NOT NULL THEN now() END)",
    )
    .bind(new_id("publication_attempt"))
    .bind(publication_id)
    .bind(result)
    .bind(reason)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(state)
}

#[derive(sqlx::FromRow)]
struct ClaimedCommit {
    #[sqlx(flatten)]
    commit: Commit,
    attempt_id: String,
}

enum CommitClaim {
    Claimed(ClaimedCommit),
    NotClaimed(PublicationState),
}

async fn commit(
    db: &PgPool,
    adapter: &impl Adapter,
    publication_id: &str,
) -> Result<PublicationState, sqlx::Error> {
    let claimed = match claim_commit(db, publication_id).await? {
        CommitClaim::Claimed(claimed) => claimed,
        CommitClaim::NotClaimed(state) => return settle(db, publication_id, state).await,
    };
    let ending = match adapter.commit(&claimed.commit).await {
        CommitResult::Published {
            remote_id,
            remote_url,
        } => Ending {
            state: PublicationState::Published,
            result: AttemptResult::Published,
            reason: None,
            remote_id: Some(remote_id),
            remote_url,
        },
        CommitResult::Refused { reason } => Ending::failed(reason),
        CommitResult::Ambiguous { reason } => Ending::outcome_unknown(reason),
    };
    let recorded = end_run(
        db,
        publication_id,
        PublicationState::Publishing,
        Some(&claimed.attempt_id),
        &ending,
    )
    .await;
    // The publication holds no record of this answer, so the log is the only
    // one, and a post it names may exist.
    if !matches!(recorded, Ok(true)) {
        tracing::error!(
            publication_id,
            result = ?ending.result,
            reason = ending.reason,
            remote_id = ending.remote_id,
            remote_url = ending.remote_url,
            "a provider commit's result was not recorded"
        );
    }
    if recorded? {
        return Ok(ending.state);
    }
    // Another execution ended this attempt, and what the publication holds now
    // may be a later run's. Answering the state this step left stops the run.
    Ok(PublicationState::Publishing)
}

// This commits before the provider is called, so a cancel racing it either
// cancels first or answers 409, per 00002 D16.
//
// A row lock's owner can roll back, and PostgreSQL then does not check a waiting
// update's conditions again. So the lock comes first, and the deadline is read
// from the clock once it is held. Holding it also keeps a missed deadline this
// run's: nothing can retry and claim the publication before it fails.
async fn claim_commit(db: &PgPool, publication_id: &str) -> Result<CommitClaim, sqlx::Error> {
    let mut tx = db.begin().await?;
    let state: PublicationState =
        sqlx::query_scalar("SELECT state FROM pipewhere.publication WHERE id = $1 FOR UPDATE")
            .bind(publication_id)
            .fetch_one(&mut *tx)
            .await?;
    if state != PublicationState::Preparing {
        tx.commit().await?;
        return Ok(CommitClaim::NotClaimed(state));
    }
    let claimed: Option<ClaimedCommit> = sqlx::query_as(concat!(
        "UPDATE pipewhere.publication p SET state = 'publishing'
         FROM pipewhere.channel c, pipewhere.publication_attempt a
         WHERE p.id = $1 AND c.id = p.channel_id
             AND a.publication_id = p.id AND a.result IS NULL
             AND clock_timestamp() <= ",
        deadline!(),
        " RETURNING c.provider, p.body, p.id AS publication_id, c.id AS channel_id,
             a.id AS attempt_id"
    ))
    .bind(publication_id)
    .fetch_optional(&mut *tx)
    .await?;
    let claim = match claimed {
        Some(claimed) => CommitClaim::Claimed(claimed),
        // With no running attempt, this fails rather than call a provider whose
        // answer nothing could record.
        None => {
            let ending = Ending::failed(MISSED_ITS_TIME);
            end_attempt_and_move(&mut tx, publication_id, state, None, &ending).await?;
            CommitClaim::NotClaimed(ending.state)
        }
    };
    tx.commit().await?;
    Ok(claim)
}

struct Ending {
    state: PublicationState,
    result: AttemptResult,
    reason: Option<String>,
    remote_id: Option<String>,
    remote_url: Option<String>,
}

impl Ending {
    fn failed(reason: impl Into<String>) -> Self {
        Self {
            state: PublicationState::Failed,
            result: AttemptResult::Failed,
            reason: Some(reason.into()),
            remote_id: None,
            remote_url: None,
        }
    }

    fn outcome_unknown(reason: impl Into<String>) -> Self {
        Self {
            state: PublicationState::OutcomeUnknown,
            result: AttemptResult::OutcomeUnknown,
            reason: Some(reason.into()),
            remote_id: None,
            remote_url: None,
        }
    }
}

/// Ends the run from the state it found, without calling the adapter. A race
/// can change that state before the ending applies, so it then reads it again.
async fn settle(
    db: &PgPool,
    publication_id: &str,
    mut state: PublicationState,
) -> Result<PublicationState, sqlx::Error> {
    loop {
        let ending = match state {
            // 00003 D6 runs one invocation per publication at a time, so this
            // is an earlier execution's claim, and that execution was
            // interrupted.
            PublicationState::Publishing => Ending::outcome_unknown(STOPPED_DURING_COMMIT),
            PublicationState::Cancelled => {
                // A publication cancelled while `scheduled` never had an attempt.
                sqlx::query(
                    "UPDATE pipewhere.publication_attempt
                     SET result = $2, ended_at = now()
                     WHERE publication_id = $1 AND result IS NULL",
                )
                .bind(publication_id)
                .bind(AttemptResult::Cancelled)
                .execute(db)
                .await?;
                return Ok(PublicationState::Cancelled);
            }
            state => return Ok(state),
        };
        if end_run(db, publication_id, state, None, &ending).await? {
            return Ok(ending.state);
        }
        state = read_state(db, publication_id).await?;
    }
}

async fn end_run(
    db: &PgPool,
    publication_id: &str,
    from: PublicationState,
    only_attempt: Option<&str>,
    ending: &Ending,
) -> Result<bool, sqlx::Error> {
    let mut tx = db.begin().await?;
    let moved = end_attempt_and_move(&mut tx, publication_id, from, only_attempt, ending).await?;
    tx.commit().await?;
    Ok(moved)
}

/// Moves the publication and ends its attempt, for one transaction per 00002
/// D19. Answers false when the publication had left `from`, or when
/// `only_attempt` names an attempt that has already ended.
async fn end_attempt_and_move(
    tx: &mut PgConnection,
    publication_id: &str,
    from: PublicationState,
    only_attempt: Option<&str>,
    ending: &Ending,
) -> Result<bool, sqlx::Error> {
    let moved = sqlx::query(
        "UPDATE pipewhere.publication SET state = $3, remote_id = $4, remote_url = $5
         WHERE id = $1 AND state = $2 AND ($6::text IS NULL OR EXISTS (
             SELECT 1 FROM pipewhere.publication_attempt WHERE id = $6 AND result IS NULL
         ))",
    )
    .bind(publication_id)
    .bind(from)
    .bind(ending.state)
    .bind(&ending.remote_id)
    .bind(&ending.remote_url)
    .bind(only_attempt)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        == 1;
    if !moved {
        return Ok(false);
    }
    // `fetch_one` fails on a publication with no running attempt, which rolls
    // the move back rather than leave a failure with no reason.
    sqlx::query(
        "UPDATE pipewhere.publication_attempt
         SET result = $2, reason = $3, ended_at = now()
         WHERE publication_id = $1 AND result IS NULL
         RETURNING id",
    )
    .bind(publication_id)
    .bind(ending.result)
    .bind(&ending.reason)
    .fetch_one(&mut *tx)
    .await?;
    Ok(true)
}
