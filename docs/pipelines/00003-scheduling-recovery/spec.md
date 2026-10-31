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
