# Plan 00002 — Domain model and publication state machine

No step starts until the operator approves the intent, except the Threads access check in step 0, which the intent asks to settle regardless.

## Prerequisites

- The intent is approved.
- Infrastructure 00001 has been built far enough to provide:
  - the local stack with PostgreSQL, RustFS, and Restate
  - Auth running Better Auth with its organization plugin, organization-owned API keys, and ids in the infrastructure D6 format, issuing tokens that carry `sub`
  - Auth's column grants to API, Auth's migrations running before API's, and API's per-request check of the organization each request names, per infrastructure D2
  - the API and Worker Rust services
  - a way to run PostgreSQL migrations
- Where 00001's build settles a convention this plan also names, such as crate names, the migration tool, or the HTTP framework, 00001 wins and this plan is corrected.

## Proposed layout

- `crates/domain`: types, the state machine, slot resolution, and validation. No I/O.
- `crates/store`: migrations and queries.
- `crates/providers`: the provider contract, failure classification, the Mastodon and Threads adapters, and a scripted fake.
- `crates/app`: the application layer shared by API and Worker. It is the only write path, per infrastructure D4.
- `apps/api`: REST handlers and the published API schema.
- `apps/worker`: Restate handlers and the recovery command.

Proposed libraries: `sqlx` for PostgreSQL, `jiff` for time zones, `reqwest` for provider calls, the Restate Rust SDK for the worker, and a mock HTTP server for classification tests. The Restate server and SDK versions are pinned independently, per infrastructure.

## Steps

Every command below is planned and has not run. The verification log at the end records what actually ran.

### 0. Settle provider behaviour

Each result is recorded in the spec's "Provider behaviour relied on" table.

- **Threads access:** decide whether an owned account can use development-mode tester access, or needs full app review. Start now, because review has lead time.
- **Threads republish:** re-issue `threads_publish` with a `creation_id` that is already published. Record whether it errors, is a no-op, or publishes again.
- **Threads status after a timeout:** abort a `threads_publish` call client-side after sending it, then read the container status. Repeat enough times to see whether `PUBLISHED` is reliable.
- **Threads character counting:** post text with emoji and URLs near the limit, and record how the provider counts them.
- **Mastodon idempotency:** create a status, abort before reading the response, then retry with the same `Idempotency-Key` at increasing delays. Record whether the original status comes back, and for how long.
- **Mastodon token expiry:** check whether each instance in scope expires access tokens.
- **Mastodon capabilities:** confirm which instance fields report limits, URL length, and media types.

A result that contradicts D9 changes the spec before steps 7 to 11 start.

### 1. State machine

Pure transition rules for D6, D7, and D19, in `crates/domain`.

- Every transition in the D6 table succeeds.
- Every other pair of states is rejected, including every exit from `published` and `cancelled`.
- Edits are accepted only in `pending_approval` and `scheduled`.
- Cancel is accepted only in `pending_approval`, `scheduled`, and `preparing`.
- A body or media change by an actor whose approval mode is `required` moves a scheduled publication to `pending_approval`, and records that actor in `submitted_by`.
- `publishing` returns to `preparing` only for a send that definitely did not happen, before the deadline.

Verified by: `cargo test -p pipewhere-domain state_machine`

### 2. Queue projection and ranks

Pure queue logic for D4.

- Ranks: a new rank sorts after the last, before the first, or between two neighbours. Repeated inserts at the same point keep producing distinct, ordered ranks.
- The projection fills slots in rank order, starting five minutes from now.
- It skips a slot instant that a pinned publication on the same channel holds.
- It skips a local time that does not exist on a daylight-saving transition day, and resolves a repeated local time to its first occurrence.
- It uses the organization's timezone.
- A queued publication beyond 52 weeks, or on a channel with no slots, has no projected time.
- A channel that is not `active` has no projected times.
- Moving one publication changes only that publication's rank.

Verified by: `cargo test -p pipewhere-domain queue`

### 3. Validation

Pure validation for D13 and D15.

- Mastodon counts a URL as the instance's fixed URL length.
- Limits come from the channel's stored capabilities, not from provider constants.
- Media count, type, and size are checked.
- Media on a Threads channel is rejected when no public object URL is configured.
- Every problem in a request is reported, per target.

Verified by: `cargo test -p pipewhere-domain validation`

### 4. Schema

Migrations for every entity in the spec, tested against the stack's PostgreSQL.

The tables below are a starting point, and the migrations replace them. Every `organization_id` is a foreign key to Auth's organization table, per infrastructure D2. Actor ids have no foreign key, per D20.

#### channel

    channel
    -------
    id
    organization_id

    provider                   mastodon | threads
    provider_host              the Mastodon instance host; the fixed API host for Threads
    provider_channel_id

    username
    display_name
    capabilities jsonb

    credentials_encrypted bytea    null once disconnected
    credentials_key_id
    credentials_expires_at         null when the provider does not expire them

    status                     active | reconnect_required | disconnected
    status_message
    status_changed_at
    created_at

    unique (provider, provider_host, provider_channel_id) where status <> 'disconnected'

#### provider_app

    provider_app
    ------------
    provider
    provider_host
    client_id
    client_secret_encrypted
    credentials_key_id
    created_at

    primary key (provider, provider_host)

The Threads app is deployment configuration and has no row.

#### posting_slot

    posting_slot
    ------------
    id
    channel_id
    weekday         1 to 7, Monday first
    local_time      in the organization's timezone

    unique (channel_id, weekday, local_time)

#### content, content_media

    content                      content_media
    -------                      -------------
    id                           content_id
    organization_id              media_id
    body                         position
    created_at                   alt_text
    updated_at
                                 primary key (content_id, position)

#### media

    media
    -----
    id
    organization_id
    storage_key
    mime_type
    size_bytes
    sha256
    created_at

#### publication, publication_media

    publication                  publication_media
    -----------                  -----------------
    id                           publication_id
    organization_id              media_id
    content_id                   position
    channel_id                   alt_text

    body            NOT NULL     primary key (publication_id, position)
    publish_at
    queue_rank
    state
    attempt         0 before the first attempt
    submitted_by    null until an actor whose mode is required submits it

    remote_post_id
    remote_url
    published_at

    created_at
    updated_at

#### publication_attempt

    publication_attempt
    -------------------
    publication_id
    number
    started_at
    commit_started_at
    finished_at
    outcome          published | failed | outcome_unknown | cancelled | superseded
    error_code
    error_message
    detail jsonb

    primary key (publication_id, number)

`commit_started_at` is null when the attempt ended before the provider commit. `detail` holds the provider handle, the provider's request id, and the provider's raw error text.

#### idempotency_key

    idempotency_key
    ---------------
    organization_id
    actor_id
    key
    request_hash
    response_status
    response_body jsonb
    created_at

    primary key (organization_id, actor_id, key)

#### actor_policy

    actor_policy
    ------------
    organization_id
    actor_id
    approval_mode    auto | required
    updated_at

    primary key (organization_id, actor_id)

Proposed id prefixes: `chan_` channel, `slot_` posting slot, `cont_` content, `med_` media, `pub_` publication. The other tables are keyed by their parent and have no id of their own.

Tests:

- A second active connection of the same provider, host, and remote id fails, and succeeds again after a disconnect.
- Media referenced by a publication cannot be deleted.
- Content referenced by a publication cannot be deleted.
- Migrations apply cleanly to a database where only Auth's migrations have run.
- A row naming an organization that does not exist in Auth is rejected.
- Deleting an organization in Auth fails while a channel references it.
- Every id column rejects a value with the wrong prefix or format, per infrastructure D6.
- Generated ids parse back to a version 7 UUID, sort in creation order to the millisecond, and never exceed 32 characters.

Verified by: `cargo test -p pipewhere-store`

### 5. Roles and the organization's timezone in Auth

Configure Auth for D14 and D18:

- Define the `owner`, `admin`, and `member` roles, with the organization-management permissions in D14. Sub-teams, custom roles per organization, and organization deletion stay off.
- Allow only admins and owners to create, change, and revoke organization-owned API keys.
- Add `timezone` to the organization. It is required when an organization is created, and must be an IANA zone name.
- Grant API's database role read access to `timezone`. A column grant does not cover columns added later.

Tests:

- Creating an organization without a valid timezone fails.
- API's database role can read an organization's timezone and a key's permissions, and cannot read a session token or a password hash.
- A member cannot create an API key, and an admin can.
- An admin cannot remove or demote an owner.

Verified by: `pnpm --filter @pipewhere/auth test organization`

### 6. Application layer

Handler-level tests with real PostgreSQL, for commands and queries in `crates/app`. The test database has Auth's migrations applied, so membership, role, key, and timezone reads run against Better Auth's real tables.

Commands:

- Create publications all-or-nothing, with a snapshot and per-target overrides.
- Add to the queue, move, pin, and re-add, generating ranks under the channel row lock.
- Send `recompute` to the channel's scheduler after every change that commits.
- Edit, cancel, retry, approve, reject, and resolve.
- Set approval modes.
- Connect, reconnect, and disconnect channels, and edit posting slots.

Queries:

- What is scheduled in a time range.
- Publications in `failed` or `outcome_unknown`, each with its latest attempt.
- Publications awaiting approval, including those whose time has passed.
- Channels that are not `active`, with the publications that will fail unless they reconnect.

Permissions, approval modes, the five-minute boundary, and idempotency keys are enforced here.

Tests:

- Editing content leaves its publications unchanged.
- A `published` publication rejects every edit.
- Two concurrent add-to-queue requests for one channel get different ranks.
- Changing posting slots updates no publication row.
- A scheduled publication cannot hold both a time and a rank.
- A replayed idempotency key sends `recompute` again.
- A cancel racing the commit claim: exactly one wins, and a losing cancel reports `publishing`.
- A client with only `publication:schedule` cannot create a publication one minute ahead.
- A client without `publication:resolve` cannot resolve an unknown outcome.
- A replayed idempotency key returns the stored response, and a changed body under the same key is rejected.
- Disconnecting cancels `scheduled` and `preparing` publications, and is refused while one is `publishing`.
- A resource in another organization is reported as not found, for reads and for writes.
- A request naming an organization the user is not a member of is reported as not found. So is a key used under an organization that does not own it.
- A user in two organizations acts in each by naming it, with the same token.
- A member's token cannot connect channels or set approval modes.
- An admin demoted to member cannot connect a channel on their next request, with the same token.
- A key can use only its own permissions, and a changed permission applies on the next request.
- An API key with no policy row lands its publications in `pending_approval`. A human with no row does not.
- A `pending_approval` publication holds no queue rank. Approving one meant for the queue puts it at the back.
- An actor cannot approve a publication it submitted, including one it edited while the publication awaited approval.
- Approving a pinned publication whose time has passed, without a new decision, is rejected with `schedule_missed`. Choosing now needs `publication:publish_now`.
- A body change by a `required` actor after approval sends the publication back to `pending_approval`.

Verified by: `cargo test -p pipewhere-app`

### 7. Provider contract and fake

The contract covers connecting, reading capabilities, refreshing credentials, preparing, committing, and reading the remote URL. Commit failures are classified as definitely not sent or unknown, per D9.

Classification tests run each adapter against a mock HTTP server:

- connection refused
- a 4xx validation error
- a 401
- a 429
- a response dropped after the request was sent
- a timeout while awaiting the response
- a 5xx with no body

The scripted fake provider is used by the workflow tests. It can be told to fail in each of these ways, and it counts every commit call.

Verified by: `cargo test -p pipewhere-providers classification`

### 8. Channel scheduler and publish workflow

The Restate handlers for D3, D5, D7, D8, and D9, tested with the fake provider and the stack's Restate.

Scheduler:

- It wakes at whichever comes first: the next pinned time, or the queue head's slot.
- It starts the queue head at its slot, writing `publish_at` and clearing the rank.
- A head moved or pinned between the scheduler's read and its claim is not started, and the scheduler recomputes.
- A wake-up from an older generation does nothing.
- A queue holds while its channel is not `active`, and resumes at the first slot after reconnecting.
- After downtime, a queued publication fires at the first slot after the system returns. A pinned publication more than 15 minutes past its time fails with `grace_period_exceeded`.
- A failed publication does not stop the next queued publication from firing at its slot.
- A slow preparation does not delay the next pinned publication on the same channel.

Publish workflow:

- The happy path ends `published`, with the remote id and URL.
- A cancel during `preparing` means the commit is never called.
- An unknown commit ends `outcome_unknown` after exactly one commit call.
- A permanent rejection ends `failed` at once, and is not retried.
- A transient send that definitely did not happen is retried until the deadline, then ends `failed` with `grace_period_exceeded`. A retry never follows an unknown outcome.
- A queued publication, once started, gets the same 15-minute deadline from its slot.
- A pinned publication whose channel is not `active` at its time fails with `channel_reconnect_required`.
- **Replay test, required by infrastructure D4.** A test-only fault hook aborts the worker process right after the fake provider accepts a commit, before the step returns. After a restart, the publication is `outcome_unknown` with `commit_interrupted`, and the fake has seen exactly one commit call.

Verified by: `cargo test -p pipewhere-worker scheduler` and `cargo test -p pipewhere-worker publish`

### 9. Recovery command

`worker recover`, and `worker recover --confirm-stopped` for the `publishing` step, per D10. Tested against a stack whose Restate volume is wiped mid-schedule.

- Every channel with `scheduled` publications gets `recompute`. A pinned publication then publishes once, and a queue resumes at its next slot.
- A `preparing` publication is taken over. A simulated surviving invocation loses its commit claim.
- A `publishing` publication is untouched by `worker recover`, and becomes `outcome_unknown` with `recovered_unknown` only after `--confirm-stopped`.
- A pinned publication re-driven more than 15 minutes after its time fails with `grace_period_exceeded`.
- Threads refresh timers are re-armed.

Verified by: `cargo test -p pipewhere-worker recovery`

### 10. Mastodon adapter

- Registers the app once per instance.
- Connects channels through OAuth.
- Reads capabilities from the instance.
- Uploads media and waits for processing, with durable sleeps.
- Creates the status with `Idempotency-Key` set to the publication id.
- Classifies failures per D9, and records the remote URL.

Verified by an opt-in live test against a test account. It is skipped unless its credentials are set.

`PIPEWHERE_LIVE_MASTODON=1 cargo test -p pipewhere-providers mastodon_live`

### 11. Threads adapter and credential refresh

- Connects channels through OAuth and exchanges for a long-lived token.
- Stores fixed capabilities.
- Creates the container and records its id before the commit.
- Polls the container status, publishes, and reads the permalink.
- Refreshes the token by timer, per D12.

Verified by an opt-in live test against a tester account:

`PIPEWHERE_LIVE_THREADS=1 cargo test -p pipewhere-providers threads_live`

The refresh timer is tested with the fake provider and a token set to expire within the threshold:

`cargo test -p pipewhere-worker refresh`

### 12. REST API

Endpoints for the nine operational questions, plus resolve, approval, and approval modes. The published API schema uses the D1 vocabulary and the D16 error codes.

Every path below is under `/v1/orgs/{organization_id}`, per D18.

| Question | Endpoint |
| --- | --- |
| Publish this now, to these channels | `POST /publications` with timing `now` |
| Schedule this for a specific time | `POST /publications` with timing `at` |
| Add this to the next posting slot | `POST /publications` with timing `queue` |
| Reorder the queue | `POST /publications/{id}/move` |
| Change or cancel this | `PATCH /publications/{id}`, `POST /publications/{id}/cancel` |
| What is scheduled this week | `GET /publications?from=&to=` |
| Did this go out, and where | `GET /publications/{id}`, `GET /contents/{id}/publications` |
| Which posts failed, and why | `GET /publications?state=failed,outcome_unknown` |
| Retry what failed | `POST /publications/{id}/retry` |
| Which channels cannot publish | `GET /channels?status=reconnect_required` |
| Resolve an unknown outcome | `POST /publications/{id}/resolve` |
| Approve or reject | `POST /publications/{id}/approve`, `POST /publications/{id}/reject` |
| Set an actor's approval mode | `PUT /actors/{id}/policy` |

Tests:

- Request and response tests for every endpoint.
- Missing scope, non-member subject, malformed input, a stale state (conflict), and a missing idempotency key.
- One test asserts that the state and error-code enums in the published schema equal the domain enums, so the names have one source.

Verified by: `cargo test -p pipewhere-api`

### 13. Acceptance

Against live Mastodon and Threads accounts, answer each of the nine operational questions through REST. Force one failure and one unknown outcome with the fault hook, and resolve both. Record the run in the verification log.

Verified by: a scripted run, `scripts/acceptance-00002.sh`, with its output kept in the log.

## Not verified by this pipeline

- The intent's criterion that names mean the same thing in MCP, CLI, and web. This pipeline ships REST only. D1 and D17 bind the pipelines that add the other interfaces.
- Infrastructure D2's checks: token exchange, the grants between the two schemas, and removal and revocation taking effect on the next request. 00001 verifies them.

## Verification log

Nothing has run yet.

| Date | Step | Command | Result |
| ---- | ---- | ------- | ------ |
