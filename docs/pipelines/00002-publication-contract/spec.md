# Spec 00002 — Publication contract

## Decisions

### D1 — One vocabulary for every interface

Binding: 00003–00006, and later API, MCP, CLI, and web pipelines that expose publications.

- An **organization** is the ownership boundary, managed by Auth.
- A **channel** links one external account to one organization, per D7.
- **Content** is private source writing, with no channel or time.
- A **publication** is one snapshot of text for one channel. It is the unit of scheduling, state, and outcome.
- A **publication attempt** records one execution and explains its failure or unknown outcome.

"Post" means the remote post created by a provider, not a Pipewhere entity. Later interfaces use the names and states in the REST API schema. A later interface violates this decision if it renames an entity or reports `outcome_unknown` as `failed` or `published`.

What would overturn this: evidence that operators or clients consistently misunderstand a term, followed by one coordinated rename across the API and its clients.

### D2 — Each publication owns its sent content

A publication copies its plain-text body when created. A request may supply text directly, creating source content in the same transaction, or refer to existing content. `content_id` groups publications made from the same writing. Editing content never edits a publication. Editing a publication changes only that channel's copy. After `published`, its body, remote id, and remote URL cannot change.

This makes the scheduled and sent record readable without reconstructing old content versions. A shared body with per-channel overrides would make edits and review harder to reason about. Media snapshots are decided when media is in scope.

### D3 — States preserve the publish boundary

Binding: 00003, 00004, 00006, and later pipelines that change a publication's state.

| State | Meaning |
| --- | --- |
| `scheduled` | Waiting for its time; editable and cancellable. |
| `preparing` | Preparing provider inputs; no public commit has started. Cancellable, but not editable. |
| `publishing` | A provider commit may have started; neither editable nor cancellable. |
| `published` | A remote post is confirmed. Terminal. |
| `failed` | The provider definitely did not publish, or work stopped before commit. Editable; leaves only by explicit retry. |
| `outcome_unknown` | A remote post may exist. No automatic retry. |
| `cancelled` | Stopped before commit. Terminal. |

After scheduling, the normal path is `scheduled` → `preparing` → `publishing` → `published`. The other transitions are:

- `scheduled` or `preparing` → `failed` or `cancelled`, before commit.
- `publishing` → `preparing`, when the provider definitely did not publish and the work may run again automatically, per D4.
- `publishing` → `failed`, when the provider definitely did not publish and the work stops, such as a rejected request or revoked credentials.
- `publishing` → `outcome_unknown`, when a remote post may exist.
- `failed` → `scheduled`, on an explicit retry. Its next run creates another attempt.
- `outcome_unknown` → `published`, when a person supplies the remote reference, or → `failed`, when a person confirms nothing was posted.

Any unlisted transition is rejected.

A failed publication is editable so that an operator can fix what the provider rejected and then retry it. Buffer, Publer, PostFast, and SmarterQueue all let an operator edit a failed post before retrying or rescheduling it. Without this, a content rejection could only be fixed by creating a new publication.

Publishing now uses the same path as a publication scheduled for the current time. The scheduler, the time a retry is scheduled for, the delivery deadline, and which provider errors may run again belong to the scheduling and adapter pipelines.

A later pipeline violates this decision if it moves a publication through an unlisted transition, edits one outside `scheduled` or `failed`, or cancels one in `publishing`. The one exception is an approval state before `scheduled`. An approval pipeline may add it through its own decision, which lists that state's transitions and says whether it is editable. That decision does not need to overturn this one.

What would overturn this: a provider whose commit cannot be isolated as one step after cancellable preparation, or evidence that operators need to edit a publication while it is `preparing` or `publishing`.

### D4 — An ambiguous commit never runs again automatically

Binding: 00004 and later pipelines that add a provider adapter or another call that creates public content.

A timeout after sending, a lost response, or an unproven server failure becomes `outcome_unknown`. Only a result known to have done nothing can be retried automatically. Recovery after a restart must not replay a provider commit whose result may already be public. A provider-specific idempotency handle may add protection, but cannot justify treating an unverified outcome as failure.

The worker records the attempt and its reason so the operator can investigate without raw server logs. The adapter pipeline must define and test its commit guard, including a crash after provider acceptance but before the result is durable. A later pipeline violates this decision if it automatically commits again after an ambiguous result.

This deliberately leaves some missed posts for manual resolution. A duplicate public post cannot be fully undone by deleting it.

What would overturn this: live tests showing that every supported provider reliably deduplicates the same commit across ambiguous failures.

### D5 — Client retries keep one publication result

Binding: 00003 and later pipelines that add a client creating or retrying publications.

Create and retry requests require an `Idempotency-Key` with at least 128 random bits. The key is unique within an organization across actors and replacement API keys. The same key and request return the stored result; the same key with different input is rejected. Concurrent use of one key produces one operation. The key, request hash, result, and publication changes commit together.

Successful keys have no time-based expiry while their publication history exists. Before returning a stored result, API checks the current caller's authorization. A client retry with a new key violates this decision because it can create a second publication. An endpoint violates it if it creates or retries a publication without an `Idempotency-Key`, or commits the key separately from the publication change. A deliberate new request with a new key may publish identical text.

What would overturn this: client-assigned publication ids that make create requests naturally idempotent.

### D6 — Organizations bound every publication request

Binding: 00003–00006 and later pipelines that add an organization-scoped API endpoint or business row.

An operator working alone uses an organization with one member; a team shares one with several. Auth owns that membership. One user may belong to several organizations, such as a personal one and a company one. Each holds its own channels, content, and publications, and nothing is shared between them. Every business row belongs to an organization directly or through its parent. Every request names one organization, and API checks the caller against it before reading or writing business records, as infrastructure D2 requires. A resource from another organization appears not found.

No request reads or writes across organizations. A user who wants to see what is scheduled in both a personal and a company organization makes one request to each.

The API path is `/v1/orgs/{organization_id}/...`. A later endpoint violates this decision if it takes the ownership scope from a body field or from the resource being accessed instead of the request's authorized organization.

What would overturn this: a need to share administration, billing, or business records across organizations, requiring another level above them.

### D7 — A channel is a link to an external account

Binding: 00004 and later pipelines that connect a provider account.

A channel records that one organization links one external account. Pipewhere does not model the account behind the link. Several organizations may link the same external account. Each link is a separate channel, and no organization learns of another's.

This keeps D6's boundary. Connecting a channel never looks at another organization's channels, so a personal and a company organization can both link one account without either seeing the other.

Organizations that link the same account share that provider's per-account limits, such as Threads' daily publish cap. One organization's volume can then make another's publication fail with the provider's error. That publication becomes `failed`, and its operator can edit or retry it.

A later pipeline violates this decision if connecting a channel checks or reveals another organization's links.

What would overturn this: evidence that accounts shared across organizations cause provider-limit failures or duplicate posts that operators cannot diagnose from within their own organization.

### D8 — A channel's provider comes from a fixed list

A channel's `provider` is one of a fixed list of names. The list starts with `mastodon` and `threads`, the first providers in the intent's constraints. The API schema publishes the list as a string enum, and the table checks the column against the same list. Adding a provider takes a migration that widens the check and a new variant in API's `Provider` enum.

This lets a client read from the schema which providers exist. The database also refuses a provider API cannot read back.

The cost falls on clients that validate the enum strictly, such as one generated into a Rust enum or a zod schema. Each new provider adds a value to the enum in responses. Until such a client is regenerated, it fails to read an organization's whole channel list once one channel uses the new provider, not only that channel.

Rejected alternatives:

- **Free text.** The schema would not say which values exist, and a misspelt provider would be stored.
- **A PostgreSQL enum type.** PostgreSQL cannot drop a value from an enum type without rebuilding it. A value added inside a transaction cannot be used until that transaction commits. A text column with a check changes with one `ALTER TABLE`.

A provider can be listed before its adapter exists. Until 00004 and 00008 build theirs, a Mastodon or Threads channel can be stored and read, but not published to.

### D9 — A request body API cannot read answers with an error code

Proposed: awaiting operator approval.

Binding: later pipelines that add an API endpoint with a request body.

Every endpoint reads its JSON body through `JsonBody` in `apps/api/src/error.rs`, not axum's `Json`. A body it cannot read answers with API's error body, `{ "code": "invalid_body" }`, and the status axum's `Json` extractor chose:

- 400 when the body is not JSON.
- 413 when the body is over axum's default limit of 2 MB.
- 415 when the request does not say `application/json`.
- 422 when the JSON has the wrong shape, or fails the endpoint's own checks.

axum's `Json` answers the same statuses with a plain-text message. A client would then need a second way to read API's errors. Keeping axum's statuses tells a client which of the four mistakes it made without adding a message field.

The caller is checked against the organization before the body is read, per 00001 D2. An outsider sending a bad body therefore gets not found, not `invalid_body`.

A later endpoint violates this decision if it takes axum's `Json` as a request extractor.

What would overturn this: clients needing more than the status to fix a bad request, such as which field failed. That would add a message or field list to the error body.

### D10 — Content needs text other than whitespace

Proposed: awaiting operator approval.

Content's `body` must hold at least one character that is not whitespace. API refuses empty or whitespace-only text with 422 `invalid_body`, on create and on edit. Whitespace is Rust's definition, the Unicode `White_Space` property.

Text is all content holds, so blank content is not writing. Mastodon and Threads both refuse a blank post, so a publication made from it could never go out. API has no way to delete content yet, so a blank row would stay.

No other limit applies. Content has no channel, so provider length limits belong to publications. Request bodies are capped at 2 MB by axum's default, per D9.

The table does not check the body. PostgreSQL's whitespace classes differ from Rust's, and a check that disagrees with API would refuse text API accepted. API is the only writer, since Worker runs the same application layer.

What would overturn this: a client that saves a draft before any text is typed, such as an editor that autosaves.
