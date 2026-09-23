# Spec 00002 — Domain model and publication state machine

## Context

V0 publishes to Mastodon and Threads. The intent's nine operational questions are the acceptance test. Every entity below traces to one of them, or to a constraint the intent names.

Three earlier decisions bind this pipeline:

- [Infrastructure D4](../00001-infrastructure/spec.md): PostgreSQL owns business records and Restate holds execution state only. This pipeline adds the first scheduled work and the first external effect, so it owns the recovery path and the replay test. D7, D8, and D10 answer both.
- [Infrastructure D2](../00001-infrastructure/spec.md): Auth owns organizations, members, roles, and API keys. On every request, API checks that the caller may act in the organization the request names. D14 defines what each role may do, [D18](#d18--the-organization-is-the-root-of-everything) the tenancy, and D20 the service actors, which are API keys.
- [Infrastructure D6](../00001-infrastructure/spec.md): every id is a short prefix and a UUIDv7.

## Entities

| Entity | Traces to |
| --- | --- |
| Organizations, members, roles, invitations, and API keys, in Auth | Every question. "One person or a team", per D18. "Service actors" in the intent's affected users, per D20. |
| `channel`, `provider_app` | "To these channels". "Which channels cannot publish right now?" |
| `posting_slot` | "Add this to the next posting slot." |
| `content`, `content_media` | "May draft" in the intent's affected users. |
| `media` | Publications with images or video. |
| `publication`, `publication_media` | Now, at a time, next slot, change or cancel, scheduled this week, did this go out. |
| `publication_attempt` | "Which posts failed, and why?" Retry without re-posting. |
| `idempotency_key` | A retried request has to be safe. |
| `actor_policy` | "Not every actor should publish unreviewed." |

## Decisions

### D1 — One vocabulary across every interface

Binding: every pipeline that adds an interface, including the MCP server, the CLI, and the web UI.

Each name is defined where it is decided: queues, pinned and queued publications, and projected times in D4, states in D6, attempts in D16, roles in D14, approval modes in D19, and actors and service actors in D20. Three names are defined only here:

- **channel**: a destination that publications go to, such as a Mastodon profile, connected to one organization.
- **content**: private writing with no channel and no time.
- **publication**: one body going to one channel at one time. It is the unit of scheduling, state, retry, and outcome.

"Post" is not an entity. In Pipewhere it means only the remote post that exists on the provider after a publication is published.

"Account" is not an entity either. It means only a provider's own user account. A channel is not always one account: one provider login can manage several destinations, such as several Facebook Pages.

Interfaces use these names verbatim in API fields, MCP tool names and arguments, CLI commands, and UI labels. The API's published schema carries the names, and the other interfaces take them from it.

A later pipeline is in violation if it exposes an entity or state under another name, or presents two states as one. The costliest case is showing `outcome_unknown` as `failed` or as `published`.

What would overturn this: evidence that operators or agents consistently misread one of these names. The remedy is a rename in every interface at once, recorded here. A local alias in one interface is still a violation.

### D2 — A publication is a snapshot for one channel

A publication's body and media list are copied when it is created. They come from content, and a create request may override either one per channel. Editing content never touches an existing publication, and editing a publication changes only that publication. Once a publication is `published`, its body, media, remote id, and remote URL never change.

Bodies are plain text, sent as written. There is no editor markup to convert at publish time.

A create request may carry text instead of a content id. It then creates the content in the same transaction, so every publication has content. `content_id` is what groups "these channels": asking whether something went out lists the publications of one content.

Content can be deleted only while no publication references it.

The operator chose per-channel copies over a shared post with per-channel overrides. A shared post would let an edit to content rewrite what is already scheduled, which the intent rules out. It would also make `content:write` a permission that changes what goes public, around the review in D19. The cost: when two channels need the same change, the operator makes it twice.

### D3 — Publishing now is scheduling at now

There is no separate immediate path. Publishing now creates a publication in `scheduled`, pinned to the server's current time, and the same scheduler starts it. One path means one set of failure modes to reason about and to test.

### D4 — Each channel has one queue, ordered by rank

Every `scheduled` publication is either pinned or queued, never both:

- **Pinned** publications have a `publish_at`. Publishing now, or choosing a time, pins a publication.
- **Queued** publications have a `queue_rank` instead, and no stored time. The rank is a fractional index that orders the channel's queue.

Adding to the queue puts a publication at the back, which answers "add this to the next posting slot". Moving it gives it a rank between two neighbours. Pinning a queued publication takes it out of the queue. Re-adding a pinned or failed publication puts it at the back.

Every queue operation changes only the publication it moves. Changing the posting slots changes no publication. Ranks are generated under the channel row lock, so concurrent adds never share one.

A queued publication's time is projected, not stored. The projection walks the channel's posting slots in the organization's timezone, starting five minutes from now:

- It skips any slot instant that a pinned publication on the same channel already holds.
- It gives each remaining slot to the next queued publication in rank order.
- A local slot time that does not exist on a daylight-saving transition day is skipped. A local time that occurs twice resolves to its first occurrence.
- It looks 52 weeks ahead. A queued publication beyond that has no projected time yet.

Reads use the projection, including "what is scheduled this week". The channel scheduler in D7 uses the same projection to decide what fires. When a queued publication fires, its slot instant is written to `publish_at` and its rank is cleared.

The operator chose a queue that moves over times fixed on entry, because a projected queue moves without re-timing rows.

What it costs: a queued publication's projected time can change without anyone touching it. The API marks such publications as queued, and an agent re-reads before relying on the time.

### D5 — A publication may publish within 15 minutes of its time, and never later

A publication has a delivery deadline 15 minutes after its `publish_at`:

- A pinned publication's `publish_at` is the time it was given.
- A queued publication gets its `publish_at` when the scheduler starts it at a slot, per D4.

Before the deadline, the scheduler may start the publication, and the workflow may retry transient failures, per D9. After it, nothing starts or retries automatically. The publication moves to `failed` with `grace_period_exceeded`, and it never publishes later on its own.

A pinned publication whose channel is not `active` at its time fails at once with `channel_reconnect_required`. Reconnecting the channel does not retry it.

A queued publication that has not started has no time yet, so it cannot be late. Its queue waits instead:

- If the system is down, the queue resumes at the first slot after it returns.
- If the channel is not `active`, its queue holds and has no projected times. It resumes at the first slot after the channel reconnects.

A failed publication never stops its queue. The next queued publication still fires at its slot.

The operator chose 15 minutes. It absorbs a worker restart or a brief provider outage. A post that goes out much later can be worse than one that does not go out: a missed post is recoverable by publishing it, and a stale one is not recoverable at all.

### D6 — States encode retry safety

- `pending_approval`: waiting for review, per D19. It keeps its intended timing and holds no queue slot. Editable and cancellable.
- `scheduled`: waiting, either pinned to `publish_at` or queued. Editable and cancellable.
- `preparing`: media fetched from the object store and handed to the provider. Nothing is public yet. Cancellable, not editable. A crash here is always safe to retry.
- `publishing`: the provider commit may be in flight. Neither editable nor cancellable. A crash here may or may not have produced a remote post.
- `published`: terminal. The remote id is recorded, and the remote URL when the provider gives one.
- `failed`: the provider definitely did not publish, the attempt ended before the commit, or the D5 deadline passed. Terminal until someone explicitly retries it.
- `outcome_unknown`: nobody knows whether a remote post exists. Never retried automatically, per D9.
- `cancelled`: terminal.

The table lists every transition. Any other is rejected.

| From | To | By |
| --- | --- | --- |
| `pending_approval` | `scheduled` | client with `publication:approve`, per D19 |
| `pending_approval` | `cancelled` | client, rejecting or cancelling |
| `scheduled` | `pending_approval` | client, a change to body or media by an actor whose approval mode is `required` |
| `scheduled` | `preparing` | channel scheduler, when a pinned time or the queue head's slot arrives |
| `scheduled`, `preparing` | `cancelled` | client |
| `preparing` | `publishing` | worker, the commit claim |
| `preparing` | `failed` | worker: the deadline passed, a pinned publication's channel is not active, media is missing, or the provider permanently rejected preparation |
| `publishing` | `published` | worker |
| `publishing` | `preparing` | worker, after a send that definitely did not happen, before the deadline |
| `publishing` | `failed` | worker, when the adapter classifies the failure as permanent, or the deadline passes |
| `publishing` | `outcome_unknown` | worker, when the outcome is unknown or the commit guard trips; recovery, per D10 |
| `failed` | `scheduled` or `pending_approval` | client retry, pinned or queued. It needs approval again if the retrying actor's mode is `required`. |
| `outcome_unknown` | `published` | client, with the remote URL |
| `outcome_unknown` | `failed` | client, confirming that nothing was posted |

The split between `preparing` and `publishing` separates a crash that is safe to retry from an ambiguous one. The retry policy reads that split.

### D7 — A channel scheduler decides when; conditional updates decide who

Each channel has a scheduler: a Restate virtual object keyed by the channel id. Restate runs one of its calls at a time.

- It keeps one pending wake-up. That is the earlier of the next pinned `publish_at` and, when the queue is not empty and the channel is `active`, the queue head's projected slot.
- On waking, it re-reads PostgreSQL and starts what is due: every pinned publication whose time has come, and the queue head if its slot has come.
- It then computes its next wake-up and schedules it.
- It starts a publication's workflow and never waits for it. A slow preparation cannot delay the rest of the channel's publications.

Every committed change to a channel's publications, posting slots, or status sends its scheduler a `recompute` message.

- `recompute` is safe to repeat. The scheduler keeps a generation number in its Restate state, and a wake-up from an older generation does nothing.
- A client's retry with the same idempotency key sends `recompute` again.
- If a `recompute` is lost after its change commits, the channel's schedule stays stale until the channel's next change, its next wake-up, or the recovery command in D10. A publication it missed then fails with `grace_period_exceeded` once its deadline has passed, per D5, instead of publishing late.

The scheduler's Restate state is its generation and its pending wake-up. Pinned times, ranks, and slots stay in PostgreSQL, per infrastructure D4.

Every transition is a conditional update in PostgreSQL on the publication's current state:

- Starting a publication moves it from `scheduled` to `preparing`, increments `attempt`, and inserts the attempt row, in one transaction.
- For the queue head, that update also requires the rank the scheduler read. It writes the slot instant to `publish_at` and clears the rank. If a client moved or pinned the head in the meantime, the update fails, and the scheduler recomputes.
- `preparing` to `publishing` requires `attempt` to equal the number the workflow holds.

A workflow that loses a claim stops. A client request that loses a race gets a conflict naming the current state. For example, a cancel that arrives after the commit claim is rejected with `publishing`.

`attempt` on the publication repeats the highest attempt number in `publication_attempt`. It is kept because each claim must be a single atomic conditional update on one row.

One scheduler per channel replaces one timer per publication. With a queue whose times move, per-publication timers would need re-arming on every change. A timer lost between the database commit and its creation would never fire, and nothing would notice.

### D8 — The commit claim guards the provider call

The provider call that creates the remote post runs inside one durable step. The step begins with the commit claim from D7. If the claim wins, the step calls the provider.

If Restate replays the step, the claim finds the publication already `publishing` under the same attempt. The step then moves the publication to `outcome_unknown` and does not call the provider. If the attempt number differs, the invocation has been superseded, and it stops without writing.

Two crashes lead to this path:

- A crash after the provider accepted, but before Restate recorded the result, gives `outcome_unknown`. It never gives a second provider call.
- A crash after the claim, but before the provider call, also gives `outcome_unknown`, although nothing was sent. The operator resolves that false unknown. It is accepted as the price of never calling twice.

This is the replay behaviour infrastructure D4 requires to be defined and tested. The test is in the plan.

### D9 — An unknown publish outcome is never retried automatically

Binding: every pipeline that adds a provider adapter, or any other provider call that creates public content.

A duplicate public post is unrecoverable in the way that counts: deleting it does not unsee it. A missed post is recoverable by publishing it. The asymmetry decides the policy.

Restate retries a failed step until it succeeds by default. For publishing, that is wrong in the expensive direction.

The adapter classifies every commit failure as one of three kinds:

- **Permanent**: the provider refused the request, and repeating it will not help. Validation rejections, unsupported media, and authorization failures. The publication moves to `failed` at once.
- **Transient, definitely not sent**: the request never reached the provider, or the provider refused it without acting on it. Connection refused, DNS failure, and a rate-limit rejection. The workflow moves the publication back to `preparing` and tries the send again until the D5 deadline.
- **Unknown**: the connection dropped after sending, a timeout while awaiting the response, or a 5xx that the adapter cannot show was refused before the provider acted. The publication moves to `outcome_unknown`.

Classification is part of the provider contract, not per-adapter discretion. Only the second kind is retried. A retried send stays in the same attempt and takes the commit claim again, so the replay guard in D8 still holds. Preparation is safe to repeat, so a transient failure while preparing is also retried until the deadline.

Where a provider offers a handle that makes a repeat safe, the adapter uses a stable one:

- The Mastodon `Idempotency-Key` is the publication id, the same on every attempt. An operator who wrongly resolves an unknown outcome as not posted, and retries within the instance's retention window, gets the existing status back instead of a second one.
- The Threads container id is recorded in the attempt's `detail` before the commit. The operator uses it to check the container when resolving an unknown outcome.

A later pipeline is in violation if either of these happens:

- An ambiguous failure can reach an automatic retry.
- A provider call that creates public content can run outside the guarded step in D8.

Leaving the commit step on Restate's default retry policy is itself the violation, and it is a silent one.

The cost: an operator resolves `outcome_unknown` by hand, and V0 pushes no alert, so the operator has to look. That is the intended trade against a duplicate post.

What would overturn this: every supported provider offering an idempotency handle whose behaviour has been verified under real ambiguous failure, not merely documented. The evidence has to be a test against the live provider.

### D10 — Recovery re-drives from PostgreSQL

Losing Restate's data loses in-flight invocations and pending timers. PostgreSQL still holds every publication. An operator-invoked recovery command re-drives work from PostgreSQL:

1. It sends `recompute` to the scheduler of every channel with `scheduled` publications.
2. It takes over every `preparing` publication. It increments `attempt`, conditional on the value it read, marks the old attempt `superseded`, and starts a new workflow. An old invocation that survived then loses its commit claim.
3. It leaves `publishing` publications alone while an old invocation might still be running.
4. On a second, explicit invocation, after the operator confirms that old invocations are stopped or gone, it moves the remaining `publishing` publications to `outcome_unknown`. The update is conditional on state and attempt. It never re-drives their commits.
5. It re-arms the credential refresh for every active Threads channel, per D12.

Publications re-driven after their D5 deadline fail with `grace_period_exceeded`. Queues resume at their next slot.

Automatic periodic reconciliation is deferred until operating evidence shows that manual recovery is not enough.

### D11 — A channel is identified by provider, host, and remote id

A Mastodon account id is unique only within its instance, so the instance host is part of the channel's identity. For Threads, the host is the fixed API host.

At most one organization holds a channel at a time. A unique index on the three, over channels that are not `disconnected`, enforces it. Connecting a channel that another organization holds is rejected.

The operator decided that one channel cannot be connected to several organizations. Two organizations holding one channel would mean two sets of credentials, and two schedules that can collide on the same public timeline. Nothing in V0 needs it.

- **`active`**: the channel can publish.
- **`reconnect_required`**: a provider call returned an authorization failure, or a credential refresh failed definitively. `status_message` says which. Scheduling to the channel is rejected until it reconnects.
- **`disconnected`**: an operator removed the channel. Its credentials are deleted and its `scheduled` and `preparing` publications are cancelled, all in one transaction. Its history stays. Disconnecting is refused while any of its publications is `publishing`.

Reconnecting the same channel in the same organization reactivates its row, so its history stays attached.

"Which channels cannot publish right now" reads `status`.

### D12 — Credentials are encrypted and refreshed before expiry

Provider credentials are encrypted with a key from deployment configuration. `credentials_key_id` names the key used, so keys can rotate without re-encrypting every row at once.

Mastodon has no central app registry. Pipewhere registers itself with each instance the first time a channel on it connects, and reuses that registration. The registration belongs to the deployment, not to an organization, and its client secret is encrypted the same way. The Threads app is deployment configuration.

Threads access tokens expire. A durable timer per Threads channel refreshes the token when seven days remain:

- A definitive refresh failure, such as a revoked grant, moves the channel to `reconnect_required`.
- A transient failure is retried until the token expires.

Mastodon tokens are not refreshed. `credentials_expires_at` stays null unless the instance reports an expiry. An authorization failure at use moves the channel to `reconnect_required`.

### D13 — Capabilities are discovered per channel and checked when scheduling

`capabilities` holds what the channel can accept:

- maximum characters
- the length a URL counts as
- maximum media per publication
- accepted media types and sizes

For Mastodon, they are read from the instance when the channel connects and when it reconnects, because each instance sets its own limits. For Threads, they are fixed values stored in the same shape.

The application layer validates every create, edit, retry, and approval against the channel's capabilities, whichever client sent it. Characters are counted the way the provider counts them. A Mastodon URL counts as the instance's fixed URL length, whatever its real length.

A request with any invalid target creates nothing and reports every problem, per target.

Capabilities can go stale if an instance lowers its limits. The provider then rejects the commit, which is definitely not sent, and the publication fails with `provider_rejected`. Reconnecting refreshes the capabilities.

### D14 — Roles, permissions, and where "now" ends

Humans and service actors share one permission vocabulary:

- A human's permissions come from their role in the organization, read on every request.
- A service actor's permissions are the permissions on its API key, per D20.

| Permission | Permits | `member` | `admin` | `owner` |
| --- | --- | --- | --- | --- |
| `content:read` | Read content and media. | yes | yes | yes |
| `content:write` | Create, edit, and delete content. Upload media. | yes | yes | yes |
| `publication:read` | Read publications and their attempts. | yes | yes | yes |
| `publication:schedule` | Create publications at least five minutes ahead. Add to, reorder, and remove from the queue. Edit, cancel, and retry to at least five minutes ahead. | yes | yes | yes |
| `publication:publish_now` | Create, retry, or approve a publication for less than five minutes ahead, including now. | yes | yes | yes |
| `publication:approve` | Approve or reject a publication awaiting approval, per D19. | yes | yes | yes |
| `publication:resolve` | Resolve `outcome_unknown` as `published` or `failed`. | yes | yes | yes |
| `channel:read` | Read channels, their status, and their posting slots. | yes | yes | yes |
| `channel:manage` | Connect, reconnect, and disconnect channels. Edit posting slots. |  | yes | yes |
| `actor:manage` | Set any actor's approval mode. |  | yes | yes |

Organization management happens in Auth and has no Pipewhere permission:

- Admins and owners invite and remove members, change roles, and edit the organization's name and timezone. They also create, change, and revoke API keys, and set each key's permissions.
- Only an owner can remove an owner, demote one, or make someone an owner. Better Auth enforces this, and keeps at least one owner.

A single operator is the owner of their organization. Roles add no step for them.

A `publish_at` earlier than five minutes from now counts as "now". A client without `publication:publish_now` cannot publish immediately by scheduling one second ahead. Five minutes is the smallest boundary that is plainly not "now". The same boundary keeps the queue projection from promising a slot about to fire.

`publication:resolve` is its own permission because a wrong "nothing was posted", followed by a retry, is how a duplicate is made. A service actor can then publish without being able to resolve.

A role says what a human may do. Whether their work needs review is approval policy, per D19, not a role. A member who should not publish unreviewed keeps the `member` role, with approval mode `required`.

### D15 — Media is uploaded once and never changes

Media is uploaded through API into the object store. The `media` row is written after the bytes are stored, and neither changes afterwards. Content and publications reference media with a position and alt text. A publication's list is its own snapshot. Media referenced by any publication cannot be deleted.

- **Missing bytes:** if the bytes are missing when a publication prepares, it fails with `media_missing`, per infrastructure D4.
- **Mastodon:** Pipewhere uploads the media while preparing. It polls until the instance has processed it, sleeping durably between polls.
- **Threads:** Threads fetches media itself, from a URL. Pipewhere hands it a short-lived presigned URL, so the object store must be reachable from the internet. A deployment that has not configured a public object URL rejects media on Threads channels at validation. Text-only publications to Threads work either way.

### D16 — Attempts are the failure record

Every run of the publish workflow writes one attempt, including a run that ends before any provider call. A `failed` or `outcome_unknown` publication is explained by its latest attempt. The publication carries no error columns of its own, so the reason exists once.

The error codes are part of the API contract:

- `grace_period_exceeded`: the D5 deadline passed before the publication was published. The attempt's detail carries the last transient error, if there was one.
- `channel_reconnect_required`: the channel could not publish.
- `media_missing`: referenced bytes are gone from the object store.
- `provider_rejected`: the provider permanently refused the request. The message carries the provider's text.
- `no_response`: sent, but the response never arrived. The outcome is unknown.
- `commit_interrupted`: the commit guard tripped on replay, per D8. The outcome is unknown.
- `recovered_unknown`: recovery moved the publication out of `publishing`, per D10. The outcome is unknown.

### D17 — Idempotency keys make client retries safe

Binding: every pipeline that adds a client, including the CLI, the MCP server, and the web UI.

Requests that create or retry publications require an `Idempotency-Key`. A key is scoped to the organization and the calling actor, and kept for 24 hours:

- The same key with the same request returns the stored response.
- The same key with a different request is rejected.
- The same key while the first request is still running returns a conflict.

A later pipeline is in violation if its client retries one of these requests with a new key. A new key turns the client's own retry into a second publication.

There is no content-level duplicate check. Both providers accept repeated text, and a deliberate repeat is legitimate.

What would overturn this: clients supplying the publication id themselves, which makes a create naturally idempotent.

### D18 — The organization is the root of everything

Binding: every pipeline that adds an API endpoint.

Pipewhere has one tenancy level, the organization. A single operator is an organization with one member. A team is an organization with several. Every business row carries `organization_id`, directly or through its parent.

- A signed-in user with no organization creates one and becomes its owner. Creating it sets its name and timezone. Nothing is created automatically, so an invited user does not collect an empty organization.
- Better Auth's sub-teams and per-organization custom roles stay off.

The organization holds every publishing setting. Its timezone resolves every channel's posting slots, per D4. A channel has no setting that overrides its organization's. The timezone is stored on the organization in Auth, and API reads it from Auth's organization table.

Every request names one organization. REST names it in the path, as `/v1/orgs/{organization_id}/...`. After infrastructure D2's check, API filters every query and every write by it. A resource in another organization is reported as not found, the same as an organization the caller cannot act in.

A later pipeline is in violation if it reads or writes a row whose `organization_id` came from anywhere other than the request's organization, such as a body parameter or the row being acted on.

What would overturn this: a customer who needs several brands under one bill or one set of admins. That would add a level above organizations.

### D19 — Approval is actor policy, and it comes before scheduling

Every actor has an approval mode, `auto` or `required`, in `actor_policy`. No row means `auto` for a human and `required` for a service actor. Admins and owners set it.

A service actor with `publication:schedule`, without `publication:publish_now`, and with approval `auto` can keep a queue filled on its own, without being able to publish immediately.

A publication goes to `pending_approval`, or stays there, when an actor whose mode is `required` does any of these:

- creates it
- changes its body or media while it is `scheduled` or `pending_approval`
- retries it after it failed

The publication records that actor in `submitted_by`. It keeps its intended timing: a pinned `publish_at`, or the queue. It holds no queue rank, so unapproved work never takes a slot.

Approving needs `publication:approve`:

- An actor cannot approve a publication whose `submitted_by` is itself.
- Approving a pinned publication whose time is at least five minutes away schedules it at that time.
- Approving a publication meant for the queue puts it at the back of the queue.
- Rejecting cancels the publication.

Approval authorizes content. It never publishes overdue work. If a pinned publication's time is less than five minutes away, or has passed, approving it needs one more decision in the same request: publish now, a new time, or the queue. Choosing now needs `publication:publish_now`. Approving without a decision is rejected with `schedule_missed`. "Awaiting approval with its time passed" is derived, not a state.

### D20 — Service actors are an organization's API keys

Agents, CI jobs, automations, and integrations act through API keys owned by the organization, per infrastructure D2. Pipewhere calls such a key a service actor. It has no table of its own.

- An actor is whoever performs an action. Its id is the `sub` of the caller's token: a user's `usr_` id or an API key's `key_` id. Actor ids have no foreign key, because one column holds both kinds.
- A service actor's permissions are its key's permissions, set in Auth, per D14.
- Disabling or deleting the key in Auth stops the service actor on the next request, per infrastructure D2.

The operator chose this over a service actor table that API keys are bound to. That table would keep one identity when a key is replaced. It would also add a binding step when a key is created, and a key that authenticates as nothing until it is bound.

What it costs: Better Auth cannot rotate a key in place, so a replacement key is a new actor. Its approval mode starts over as `required`, until an admin sets it again.

## Deferred

| Deferred | Returns when |
| --- | --- |
| A shared post with per-channel overrides (D2) | Organizations routinely post one piece to many channels and edit it after scheduling |
| Pausing a queue (D4) | An operator needs to hold a queue without cancelling it |
| Several named queues per organization (D4) | An organization needs different cadences for different kinds of content |
| A spacing rule around pinned publications (D4) | A pinned publication should also use up a nearby slot, not only one at the same instant |
| A configurable grace period (D5) | The 15-minute deadline fails or delays something an operator wanted otherwise |
| One channel in several organizations (D11) | A hosted customer needs it |
| A connection shared by several channels | A provider where one login manages several destinations, such as Facebook Pages. Until then, each channel holds its own credentials. |
| Richer actor policy: allowed channels, daily limits, approval per channel, and multi-step approval (D19) | An organization needs to limit a trusted actor more finely than one approval mode allows |
| A service actor identity separate from its API key (D20) | A second way to authenticate a service actor exists, or an actor must keep its approval mode when its key is replaced |
| Dated drafts | A client needs to hold a channel and time without committing to publish |
| Sub-groups inside an organization, and per-channel permissions | A member must publish to some channels and not others. Until then, separate organizations do the job. |
| Per-channel timezones (D18) | A channel's audience is in a different zone from its organization |
| A history of who changed what, and webhooks | An operator needs to know who changed a publication, or an integration needs to hear about failures, unknown outcomes, or pending approvals as they happen |
| Automatic reconciliation of unknown outcomes | A provider's status read is verified to settle unknown outcomes reliably |
| Periodic credential checks | A revoked channel is found only when its publication fails |
| Provider options such as Mastodon visibility, content warning, and language | An operator needs anything other than the channel's own defaults on the provider |
| Media dimension and duration checks | Provider rejections for them become common |

## Provider behaviour relied on

None of this has been verified against a live provider. The plan verifies each item before the adapter that depends on it ships.

| Behaviour | Relied on by |
| --- | --- |
| Mastodon honours `Idempotency-Key` on status creation, and keeps keys for about an hour. | D9 |
| Mastodon media can finish processing after the upload returns, and must be polled before a status can use it. | D15 |
| A Mastodon instance reports its character limit, URL length, media limits, and accepted media types. | D13 |
| Threads counts emoji and URLs in a way the adapter can reproduce. | D13 |
| Threads publishes in two steps: create a container, then publish it by id. | D8, D9 |
| A Threads container reports `IN_PROGRESS`, `FINISHED`, `ERROR`, `EXPIRED`, or `PUBLISHED`. It expires if unpublished for 24 hours. | D9 |
| Threads fetches media from a public URL. | D15 |
| Threads long-lived tokens last about 60 days, and can be refreshed once they are a day old. | D12 |
| Threads caps an account at 250 published posts per 24 hours. | D9 |
