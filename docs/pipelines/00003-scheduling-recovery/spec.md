# Spec 00003 — Scheduling and recovery

## Decisions

### D1 — Late work fails after a configurable tolerance

Binding: 00006, 00007, and later pipelines that start a provider commit or set a publication's time.

This answers the open question "How late may a provider commit start after its planned time?"

A publication may start its provider commit up to its lateness tolerance after its planned time. Past that, it does not publish. It becomes `failed`, with a reason saying it missed its time.

- The default tolerance is 30 minutes.
- An organization can change its default.
- A single publication can override it, for example with a short tolerance for a time-critical post.
- A tolerance is at least 5 minutes and at most 24 hours. A request outside that range is rejected.

Publishing now uses the same rule, because its planned time is the time of the request, per [publication contract D3](../00002-publication-contract/spec.md).

A late failure is an ordinary `failed` publication. Publication contract D3 lets the operator edit it and retry it explicitly. A retry chooses a new planned time and gets a new deadline. A post that goes out long after its moment can do more harm than a missed one, so sending it late stays the operator's choice.

The 24-hour cap bounds how stale a post can be when it goes out. It also bounds how far back recovery must look for work that may still run.

A later pipeline is in violation if it starts a provider commit after the publication's deadline, or lets a tolerance fall outside 5 minutes to 24 hours.

What would overturn this: operators routinely retrying late failures unchanged, which would show the default is too strict.

### D2 — The tolerance is fixed when a publication is created, and the deadline is checked when the commit starts

A publication records its tolerance when it is created. It takes the organization's default unless the request overrides it. Changing the organization's default affects new publications only. The deadline is the planned time plus that tolerance, and the API shows it for each waiting publication. For a pinned time, the deadline does not move unless the operator edits the publication. A projected time from posting slots, and so its deadline, can move; [intent 00007](../00007-posting-slots/intent.md) owns that.

The deadline applies to the start of the provider commit, the move to `publishing`. The worker checks it in the same conditional update that claims the commit. Work still in `preparing` at the deadline therefore fails before anything public happens. A commit that started in time is never cut off.

A recovery run applies the same check. Work found past its deadline after an outage becomes `failed` and does not publish.

This follows [publication contract D2](../00002-publication-contract/spec.md): a publication's record says what it will do without consulting settings that may have changed since.

### D3 — A publication whose channel cannot publish fails at its planned time

Binding: 00004, 00008, and later pipelines that add a provider adapter or change a channel's status.

D4 narrows this decision's wording. Read the two together.

This answers the open question "What happens to a waiting publication when its channel cannot publish?"

When a publication's time comes and its channel cannot publish, the publication becomes `failed`. The reason names the channel's problem, such as revoked access or an unreachable instance. Pipewhere does not hold the publication until the channel recovers. Reconnecting the channel does not resume it. The operator retries it explicitly, per [publication contract D3](../00002-publication-contract/spec.md), and the retry gets a new planned time and deadline under D1.

The failure must not be the first sign of trouble. While a publication waits, the API shows when its channel cannot currently publish, and why. The operator can then fix the channel before the planned time. The channel's readable status comes from [intent 00004](../00004-provider-delivery/intent.md).

Holding work for a broken channel would publish it at a time nobody chose once the channel returned. Failing keeps the decision with the operator. It also matches D1, where late work fails rather than going out stale.

A later pipeline is in violation if it:

- publishes a publication whose channel could not publish at its planned time, without an explicit retry
- resumes publications automatically when a channel reconnects
- hides a waiting publication's broken channel until the publication fails

What would overturn this: operators routinely retrying every publication a broken channel failed, unchanged, after reconnecting. That would show holding the work, or resuming it on reconnect, serves them better.

### D4 — D3's check reads the channel's status when the commit is claimed

Binding: 00004, 00008, and later pipelines that change a channel's status.

D3 fails a publication whose channel "cannot publish" at its planned time. This decision says what that means, so that a brief provider failure and a broken channel are not confused.

- "Cannot publish" means the channel's status says it cannot publish. Examples are revoked access, a removed account, or a channel the operator disconnected. That status comes from [intent 00004](../00004-provider-delivery/intent.md).
- The worker reads that status when it claims the publication's commit, in the same conditional update as D2's deadline check. In normal running, that moment is the planned time. After an outage, it is when the worker resumes, still within the deadline.
- Pipewhere keeps no history of channel availability. A provider outage that no status recorded does not fail a publication after the fact.
- A provider call that fails during the commit is not a channel status. It follows [publication contract D3 and D4](../00002-publication-contract/spec.md):
  - A result known to have done nothing may run again before the deadline.
  - A result known to have done nothing that should not run again, such as a rejected request or revoked credentials, becomes `failed` with the provider's reason.
  - An ambiguous result becomes `outcome_unknown`.

This keeps D3's behaviour for a broken channel and lets D1's tolerance absorb a brief failure. Without it, the builders of 00004 and 00008 could not tell a transient error from a broken channel. Recovery would also need a record of the channel's state at a moment when nobody checked it.

D4 narrows two parts of D3's wording:

- "At its planned time" in D3 means at the claim.
- An unreachable instance, one of D3's examples, fails a publication only when the channel's status records it.

D3's text can be folded together with D4 when either is next revised.

A later pipeline is in violation if it:

- fails a publication before its commit because of its channel, on anything other than the channel's status read at the claim
- sets a status that stops a channel publishing because of a failure that does not show the channel itself is broken, such as a timeout or a provider server error

What would overturn this: provider failures that last longer than D1's tolerance, and recur too often for operators to retry by hand.

### D5 — Worker finds due work by polling PostgreSQL

Worker's scheduler polls PostgreSQL every 5 seconds for publications that are `scheduled` and whose planned time has passed. It sends each one's run to Restate, up to 100 a tick, oldest planned time first. Restate holds no timer for the schedule.

Restate timers were rejected:

- API would write the publication to PostgreSQL and the timer to Restate. A send lost after the commit misses the post silently, unless API keeps an outbox.
- API could not create or retry a publication while Restate is down.
- Each edit of a planned time leaves the old timer behind, to fire and find nothing to do.
- A time pinned years ahead keeps a timer in Restate across every upgrade in between.
- Losing Restate loses every timer. Recovery would re-drive every `scheduled` publication anyway, which is what polling does on every tick.

With polling, losing Restate loses no scheduled work, per [infrastructure D4](../00001-infrastructure/spec.md). The next tick finds it.

The costs:

- A publication starts up to 5 seconds after its planned time. D1's shortest tolerance, 5 minutes, is 60 ticks.
- A query every 5 seconds, 17,280 a day. A partial index on `planned_at` over `scheduled` publications serves it, and stays as small as the waiting work.

A tick can send a publication's run again before the first send has claimed it. That is harmless. D6 runs at most one invocation per publication at a time, and the loser of the claim stops.

### D6 — A run's journal is a sequence of PostgreSQL steps

Binding: 00004, 00008, and later pipelines that change Worker's handlers.

This is the stated way for in-flight invocations to survive a Worker upgrade that [infrastructure D14](../00001-infrastructure/spec.md) requires.

A publication runs as a Restate virtual object keyed by its id, through one exclusive handler, `run`. Restate runs at most one invocation per key at a time, so two invocations never run one publication at once.

The handler's journal holds only `ctx.run` entries named `step`:

- Each step is one PostgreSQL transaction. It reads the publication, makes at most one [publication contract D3](../00002-publication-contract/spec.md) transition through a conditional update, and returns the publication's new state.
- The handler takes steps until one returns a state the run does not continue from.
- The handler has no sleep, call, or state entry.

On an upgrade, Restate replays each recorded step from its stored state, and the next step runs live on the new code, reading PostgreSQL. A change to what a step does is therefore safe under an invocation in flight. A change to the journal's shape is not: a new kind of entry, a renamed step, or a changed return type. A pipeline making one must also ship Restate's versioned deployments, or a drain that waits for runs in flight to end, and record which in its spec.

A step interrupted before Restate records it runs again. Every step before the commit is safe to repeat. The commit step treats finding its own claim as an ambiguous commit, per D8.

A later pipeline is in violation if it adds a different kind of entry to a run's journal, such as a sleep between re-runs, without versioned deployments or a drain recorded in its spec.

What would overturn this: a run that must wait inside Restate, such as a backoff between provider re-runs, often enough that a drain is impractical.

### D7 — An attempt starts when a run claims its publication

[Publication contract D19](../00002-publication-contract/spec.md) left when an attempt starts, and how a cancel in `preparing` ends one, to this pipeline.

A run's first step claims the publication. In one transaction it moves the publication from `scheduled` and inserts the run's attempt:

- Within its deadline, the publication moves to `preparing`, and the attempt is running.
- Past its deadline, the publication moves to `failed`. The attempt starts and ends at that moment, `failed`, with a reason saying it missed its time. This is D19's failure found before the publication was prepared.

The run's later steps end that same attempt. A run records one attempt, per publication contract D20.

A cancel stays in API, unchanged. API does not end the attempt, because D19 has only the worker write attempts. The run's next step finds the publication `cancelled` and ends its attempt with a new result, `cancelled`. It means what D3's state of that name means for one execution: stopped before its commit. It needs no reason. `failed` was rejected because an operator reading a cancelled publication's attempts would see a failure nobody had.

Adding `cancelled` widens `AttemptResult`, with publication contract D8's cost: a client that validates the enum strictly fails to read it until regenerated.

Tolerated: a publication cancelled between steps, whose run then never takes another, keeps a running attempt. That needs Restate's state lost in the same moment. D9's recovery ends it.

### D8 — The commit step claims the commit, then calls the channel's adapter

The commit step does three things, in order:

1. One transaction moves the publication from `preparing` to `publishing`, on condition that its deadline has not passed. It commits before the provider is called, so a cancel racing the claim either cancels first or answers 409, per publication contract D16.
2. The step calls the channel's provider adapter.
3. A second transaction records the result and ends the attempt with it:
   - Published: `published`, with the remote id and address.
   - Refused, known to have done nothing: `failed`, with the provider's reason.
   - Ambiguous: `outcome_unknown`, with a reason.

When the claim changes no row, the step reads the publication and stops without calling the adapter:

- Still `preparing`, past its deadline: `failed`, with a reason saying it missed its time.
- `cancelled`: the attempt ends `cancelled`, per D7.
- Already `publishing`: an earlier execution of this step claimed the commit and was interrupted, by a crash, an upgrade, or lost Restate state. The commit may be public. The publication becomes `outcome_unknown`, with a reason saying Pipewhere stopped while the commit may have started. D6 makes this reading safe: no other invocation can hold the claim.

The re-run D3 allows, `publishing` → `preparing` for a result known to have done nothing that may run again, is left to 00004. Which provider errors qualify, and how long to wait between re-runs, are provider questions. A wait inside Restate also changes the journal's shape under D6.

00003 has no adapter. A channel whose provider has none refuses at the commit: the publication becomes `failed`, with a reason saying Pipewhere cannot publish to that provider yet. Nothing was sent, so it is a refusal known to have done nothing. On a running stack, every publication that reaches its time fails this way until 00004 adds Mastodon's adapter, as [publication contract D8](../00002-publication-contract/spec.md) says of a provider listed before its adapter. Tests drive the other results through a fake adapter.

Checking for an adapter before the claim was rejected. D4 forbids failing a publication before its commit because of its channel, on anything other than the channel's status.

### D9 — `worker recover` re-drives every run in flight

This is the recovery path [infrastructure D4](../00001-infrastructure/spec.md) gives the first pipeline with scheduled work.

`worker recover` is a one-off command. It finds every publication with a running attempt and sends its run to Restate. A running attempt marks a run in flight, whatever the publication's state. `scheduled` publications need no recovery, since D5's poller finds them.

The re-driven run's first step reads the publication:

- `preparing`: the run continues. Nothing public has happened.
- `publishing`: the publication becomes `outcome_unknown`, per D8. The commit is never repeated, per publication contract D4.
- `cancelled`: the attempt ends `cancelled`, per D7.

A re-drive is safe while the original invocation is still live. Restate queues it behind the original, per D6. When it runs, the attempt has ended, and it stops.

An operator runs it in two cases:

- after Restate's state is lost
- after killing an invocation that a Worker change broke, per D6

It is a command rather than part of every tick. A poller re-driving runs in flight would queue another invocation behind a stuck one on every tick, and would hide the failure that stuck it.

### D10 — The API lists overdue publications and shows whether the scheduler has stalled

The intent's second and third outcomes need two reads.

- **Overdue.** `GET /v1/orgs/{organization_id}/publications?overdue=true` lists the organization's `scheduled` publications whose planned time has passed, newest first and paged as publication contract D14 pages the list. It is the first filter that D14 left for a client that needs one. A publication past its deadline that the worker has not yet failed is still listed, since it has not started.
- **Stalled.** `GET /v1/orgs/{organization_id}/scheduler` answers when the poller last completed a tick, and whether it has stalled: no completed tick in the last 60 seconds, 12 ticks. A tick completes when its query and every send in it succeed. The poller records it in a one-row table, `pipewhere.scheduler_tick`.

The tick is the installation's, not an organization's. It reveals nothing about any organization, so any member or key of any organization may read it. The path names an organization so the request is checked against one, per publication contract D6.

A stalled tick was chosen over deriving a stall from overdue work. Overdue work shows a stopped worker only once something is due, which is when the post goes missing. The tick shows it within a minute, with nothing due.

Tolerated: a poller whose sends Restate accepts, while Restate cannot reach Worker, keeps ticking. Its due work then shows as overdue.

### D11 — An organization's default tolerance is a setting; a publication's tolerance is a field

- **The default.** `pipewhere.organization_setting` holds one row per organization, keyed by its id, with no id of its own, per [infrastructure D6](../00001-infrastructure/spec.md). Its `tolerance_seconds` is the organization's default. An organization without a row uses 30 minutes. `GET` and `PATCH /v1/orgs/{organization_id}/settings` read and change it.
- **Who may change it.** Any member and any key of the organization, like every action so far. Gating it would be the first action a role may not perform, which [infrastructure D10](../00001-infrastructure/spec.md) ties to building forbidden. Team permissions are [intent 00006](../00006-team-review/intent.md)'s.
- **A publication's tolerance.** `tolerance_seconds` on create overrides the default. The publication stores the tolerance it was created with, per D2. A publication response shows `tolerance_seconds` and `deadline`, the planned time plus the tolerance. The deadline is computed when read, not stored, so it cannot disagree with the time and tolerance it comes from.
- **Edit and retry.** An edit may change `tolerance_seconds` wherever it may change the planned time, in `scheduled` and `failed`. D2 stops a publication from reading the organization's default later. It does not stop the operator from changing the publication itself. A retry keeps the tolerance.
- **The range.** 300 to 86,400 seconds, per D1. API refuses other values with 422 `invalid_body`, and both tables check the same range.
- **Existing publications.** The migration gives each one 1,800 seconds, per publication contract D12.

Seconds, because they are the unit JSON APIs most often use for a duration, and the field's name says so.

### D12 — The channel-status check waits for 00004

D3 and D4 need a channel's status, and the intent's fourth outcome shows it on a waiting publication. That status comes from [intent 00004](../00004-provider-delivery/intent.md), which is still a draft.

The operator chose to build the rest of 00003 first. The channel-status part is 00003's last issue, built once 00004 gives a channel a readable status. 00003 stays `in-progress` until it lands. Neither intent changes.

00003 does not add a channel status of its own. One built here would be a second model of a channel's availability for 00004 to reconcile.

### D13 — Create's request hash takes the tolerance only when the request sends one

Proposed: awaiting operator approval.

[Publication contract D13](../00002-publication-contract/spec.md) hashes create's parsed request. A request that sends `tolerance_seconds` hashes it with the other fields. A request that omits it hashes exactly as it did before the tolerance existed. It does not hash as the 1,800 seconds create fills in.

- [Publication contract D5](../00002-publication-contract/spec.md) is binding on this pipeline. A key has no time-based expiry, and the same key and request return the stored result. Before #26, create refused unknown fields, so no stored key was sent with a tolerance. Every stored key therefore keeps matching its request.
- A request that omits the tolerance takes the organization's default once #27 adds it. Repeating that request after the default changes is still the same request, so it must match.
- An omitted tolerance and an explicit 1,800 are therefore different requests. One key sent with both answers 422 `idempotency_key_reused`.
- `tolerance_seconds: null` is an omitted tolerance, as [publication contract D15](../00002-publication-contract/spec.md) tolerates for `planned_at: null`. It hashes as omitted, create fills in 1,800, and edit keeps the current tolerance.

### D14 — A deadline after the year 9999 is tolerated

Proposed: awaiting operator approval.

[Publication contract D12](../00002-publication-contract/spec.md) accepts a planned time up to the end of 9999 in UTC. A tolerance can carry its deadline into 10000. The API then answers the deadline as chrono writes such a year, `+10000-01-01T00:29:59Z`. That is outside the `date-time` format the schema publishes. A create planned for `9999-12-31T23:59:59Z` with the default tolerance answered exactly that.

This is tolerated:

- **Likelihood.** It needs a planned time in the last 24 hours of the year 9999.
- **Impact.** A client that parses `deadline` strictly fails on any response that holds that publication. A list page holds up to 100 publications, so such a client loses the whole page, including its `next_cursor`. A client that reads the JSON without checking the format is unaffected. PostgreSQL holds such a time, so a check of the deadline in SQL, as D2 places it, still works.
- **Recoverability.** While the publication is `scheduled` or `failed`, the operator edits the planned time to an earlier one. Once it is `cancelled`, the API cannot change it, and it stays in the organization's list. Removing it then needs a change in the database.
- **Cost.** Keeping the deadline in range means refusing planned times in the last day of 9999. That narrows D12's accepted range, an approved decision of 00002. Doing it per publication needs the edit to check its stored time against its new tolerance, or the reverse, inside the update.

What would bring it back: a client that sends far-future times as a sentinel, such as `9999-12-31T23:59:59Z` for "never".
