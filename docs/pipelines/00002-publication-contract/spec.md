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

### D8 — The REST API schema is a hand-written OpenAPI 3.1 document in API

Binding: 00003, and every later pipeline that implements or changes an API endpoint.

Proposed: awaiting operator approval.

The schema is `apps/api/openapi.yaml`. It is the contract the intent asks for: every entity, state, transition, and error a client sees. `pnpm api:lint` checks it with Redocly CLI's strict ruleset, which fails on any warning.

It is written by hand, not generated from API's code. The contract comes before its implementation, and almost none of it is implemented yet. Generating it from Rust types, such as with utoipa, would need handlers that 00003 and later pipelines will write. Each endpoint API does not serve yet carries `x-pipewhere-status: planned`. The one it does serve, `GET /v1/orgs/{organization_id}`, was checked against API's code by reading it.

A hand-written schema can drift from the code. The first pipeline that implements a planned endpoint, expected to be 00003, adds a test that checks API's responses against the schema. Until then, nothing but review keeps them together.

A later pipeline violates this decision if it serves an endpoint, or changes one, without the schema saying the same in the same change, or removes `planned` from an endpoint before API serves it.

What would overturn this: the schema and the code drifting despite the response test, or a code-first generator that can describe endpoints before their handlers exist.

### D9 — A request creates one publication, for one channel

Proposed: awaiting operator approval.

`POST /v1/orgs/{organization_id}/publications` creates one publication, for one channel. It carries either `body`, which creates new content in the same transaction, or `content_id`. To send the same writing to three channels, a client makes three requests: the first with the text, the next two with the content id the first returned.

This is the request D2 describes. Each request has its own `Idempotency-Key`, per D5. If the connection drops after the second request, the client retries the third with its own key, and nothing is created twice.

The alternative was one request creating publications for several channels in one transaction. It would make a set of channels all-or-nothing. But one invalid channel would then refuse the whole set, and the response would need a result per channel. One publication per request keeps the publication as the unit of scheduling, state, and outcome, as D1 defines it. A batch endpoint can be added later without changing this one.

`planned_at` sets the publication's time. Without it, the publication is planned for the time of the request, per D3. Deadlines and late work belong to 00003.

Content has its own endpoints to write, read, list, and edit it, since D2 lets a publication refer to existing content. Deleting content is not in this contract. A deleted content row would leave its publications' `content_id` pointing nowhere, and nothing needs deletion yet.

Channels have read-only endpoints with only an id, a provider, and a creation time. Connecting a channel, and its ability to publish, belong to 00004, which adds their fields.

What would overturn this: clients routinely needing several channels to succeed or fail together.

### D10 — Clients move a publication only through edit, cancel, retry, and resolve

Binding: 00003, 00006, and later pipelines that add a client action on a publication.

Proposed: awaiting operator approval.

D3's transitions split between the client and the worker. A client causes only these:

| Request | Allowed from | Result |
| --- | --- | --- |
| `POST .../publications` |  | `scheduled` |
| `PATCH .../publications/{id}` | `scheduled`: body and `planned_at`; `failed`: body only | unchanged state |
| `POST .../cancel` | `scheduled`, `preparing` | `cancelled` |
| `POST .../retry` | `failed` | `scheduled` |
| `POST .../resolve` | `outcome_unknown` | `published` or `failed` |

Any other request is refused with `invalid_state`, which names the current state. The worker causes every other transition in D3.

- **Edit in `failed`** changes the body only. The publication leaves `failed` only by retry, per D3, so a new time there would mean nothing until the retry, which sets its own.
- **Cancel** on a publication already `cancelled` returns it unchanged, so a client retrying a lost cancel response does not see an error. It needs no `Idempotency-Key`: a second cancel can only find the publication cancelled, or past cancelling.
- **Retry** takes an optional `planned_at`, defaulting to the time of the request. It requires an `Idempotency-Key`, per D5.
- **Resolve** to `published` requires the remote post's id. It records who resolved the publication and when, so a later reader knows a person, not the provider, confirmed the outcome. Resolve needs no `Idempotency-Key`, since a second resolve finds the publication no longer in `outcome_unknown`.

An edit has no version check. Two people editing one publication at once can lose one edit. The worker's transitions cannot be lost this way, because each edit checks the state in the same transaction that writes it.

A later pipeline violates this decision if it lets a client cause a transition outside this table.

What would overturn this: operators losing edits to concurrent editing, which would add a version check to `PATCH`.

### D11 — Every provider response is its own attempt

Binding: 00003, 00004, and later pipelines that run a publication.

Proposed: awaiting operator approval.

An attempt is one execution of a publication: preparing its inputs, then the provider commit, up to a result. It ends as one of:

- `published`, when the remote post is confirmed.
- `retried`, when the provider definitely did not publish and the work runs again automatically. The publication goes back to `preparing`, per D3, and the next run is a new attempt.
- `failed`, when the provider definitely did not publish, or the work stopped before the commit.
- `outcome_unknown`, when a remote post may exist.

Every attempt but a published one carries a reason, with a stable `code` and a human-readable `message`. The scheduling and provider pipelines define the codes.

A publication that fails before any commit also records an attempt. One example is 00003's D3: a publication whose channel cannot publish at its planned time fails then. Its attempt holds the reason, so every `failed` and `outcome_unknown` publication has an attempt that explains it.

Ending an attempt at each automatic rerun means every provider response is readable as its own record. One attempt spanning several automatic reruns would keep only the last response, and the operator would lose why the earlier ones were refused.

A later pipeline violates this decision if a publication becomes `failed` or `outcome_unknown` without an attempt that gives the reason.

What would overturn this: automatic reruns so frequent that one attempt per response floods the record.

### D12 — Errors carry a code, and the field or state at fault

Binding: every later pipeline that adds an API error.

Proposed: awaiting operator approval.

An error response is JSON with a `code`, as infrastructure's slice 2 began. This contract makes those codes final, and adds four:

| Code | Status | When |
| --- | --- | --- |
| `invalid_request` | 400 | malformed input, with `field` naming the field at fault when there is one |
| `invalid_state` | 409 | a transition D10 does not allow, with `state` naming the current state |
| `request_in_progress` | 409 | another request with the same `Idempotency-Key` is still running |
| `idempotency_key_reused` | 422 | the `Idempotency-Key` was used for a different request |

A body that names a channel or content outside the organization is `invalid_request`, with the field. It reads the same as one that does not exist, so D6's boundary holds.

D5 says concurrent use of one key produces one operation. The second request is refused with `request_in_progress`, rather than held until the first finishes, so API holds no request open on another's behalf. The client sends it again and gets the stored result.

`forbidden` is not in this contract. Infrastructure D10 assigns it to the first pipeline with an action some role or key may not perform.

A later pipeline violates this decision if it returns an error without a `code`, or reuses a code for a different meaning.

What would overturn this: clients needing several errors per response, such as every invalid field at once.

### D13 — Channels, content, publications, and attempts take the prefixes `chn_`, `cnt_`, `pub_`, and `att_`

Binding: 00003, and the pipeline that adds each table.

Proposed: awaiting operator approval.

Infrastructure D6 makes every id a prefix and a UUIDv7. The schema's id patterns need these prefixes now, so a client can validate ids before the tables exist:

| Prefix | Entity              |
| ------ | ------------------- |
| `chn_` | channel             |
| `cnt_` | content             |
| `pub_` | publication         |
| `att_` | publication attempt |

None is one of Auth's. The table that holds each one registers its prefix with API's id generator, per infrastructure D11.

A later pipeline violates this decision if it gives one of these entities another prefix.

What would overturn this: nothing expected. A prefix is never reused, so changing one means a migration of every stored id.
