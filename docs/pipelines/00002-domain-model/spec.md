# Spec 00002 — Publication contract

## Decisions

### D1 — One vocabulary for every interface

Binding: 00003–00006, and later API, MCP, CLI, and web pipelines that expose publications.

Proposed: awaiting operator approval.

- An **organization** is the ownership boundary, managed by Auth.
- A **channel** is one publishing destination in an organization.
- **Content** is private source writing, with no channel or time.
- A **publication** is one snapshot of text for one channel. It is the unit of scheduling, state, and outcome.
- A **publication attempt** records one execution and explains its failure or unknown outcome.

"Post" means the remote post created by a provider, not a Pipewhere entity. Later interfaces use the names and states in the REST API schema. A later interface violates this decision if it renames an entity or reports `outcome_unknown` as `failed` or `published`.

What would overturn this: evidence that operators or clients consistently misunderstand a term, followed by one coordinated rename across the API and its clients.

### D2 — Each publication owns its sent content

Proposed: awaiting operator approval.

A publication copies its plain-text body when created. A request may supply text directly, creating source content in the same transaction, or refer to existing content. `content_id` groups publications made from the same writing. Editing content never edits a publication. Editing a publication changes only that channel's copy. After `published`, its body, remote id, and remote URL cannot change.

This makes the scheduled and sent record readable without reconstructing old content versions. A shared body with per-channel overrides would make edits and review harder to reason about. Media snapshots are decided when media is in scope.

### D3 — States preserve the publish boundary

Proposed: awaiting operator approval.

| State | Meaning |
| --- | --- |
| `scheduled` | Waiting for its time; editable and cancellable. |
| `preparing` | Preparing provider inputs; no public commit has started. Cancellable, but not editable. |
| `publishing` | A provider commit may have started; neither editable nor cancellable. |
| `published` | A remote post is confirmed. Terminal. |
| `failed` | The provider definitely did not publish, or work stopped before commit. Explicit retry only. |
| `outcome_unknown` | A remote post may exist. No automatic retry. |
| `cancelled` | Stopped before commit. Terminal. |

After scheduling, the normal path is `scheduled` → `preparing` → `publishing` → `published`. `scheduled` or `preparing` may become `failed` or `cancelled` before commit. `publishing` may become `failed` only when nothing was sent, return to `preparing` only when a send definitely did not happen, or become `outcome_unknown` when the result is ambiguous. An explicit retry of `failed` creates another attempt. A person may resolve `outcome_unknown` to `published` with the remote reference or to `failed` after confirming nothing was posted. Any unlisted delivery transition is rejected. A later approval pipeline may add a state before `scheduled` through its own decision.

Publishing now uses the same path as a publication scheduled for the current time. The exact scheduler and delivery deadline belong to the scheduling pipeline.

### D4 — An ambiguous commit never runs again automatically

Binding: 00004 and later pipelines that add a provider adapter or another call that creates public content.

Proposed: awaiting operator approval.

A timeout after sending, a lost response, or an unproven server failure becomes `outcome_unknown`. Only a result known to have done nothing can be retried automatically. Recovery after a restart must not replay a provider commit whose result may already be public. A provider-specific idempotency handle may add protection, but cannot justify treating an unverified outcome as failure.

The worker records the attempt and its reason so the operator can investigate without raw server logs. The adapter pipeline must define and test its commit guard, including a crash after provider acceptance but before the result is durable. A later pipeline violates this decision if it automatically commits again after an ambiguous result.

This deliberately leaves some missed posts for manual resolution. A duplicate public post cannot be fully undone by deleting it.

What would overturn this: live tests showing that every supported provider reliably deduplicates the same commit across ambiguous failures.

### D5 — Client retries keep one publication result

Binding: 00003 and later pipelines that add a client creating or retrying publications.

Proposed: awaiting operator approval.

Create and retry requests require an `Idempotency-Key` with at least 128 random bits. The key is unique within an organization across actors and replacement API keys. The same key and request return the stored result; the same key with different input is rejected. Concurrent use of one key produces one operation. The key, request hash, result, and publication changes commit together.

Successful keys have no time-based expiry while their publication history exists. Before returning a stored result, API checks the current caller's authorization. A client retry with a new key violates this decision because it can create a second publication. A deliberate new request with a new key may publish identical text.

What would overturn this: client-assigned publication ids that make create requests naturally idempotent.

### D6 — Organizations bound every publication request

Binding: 00003–00006 and later pipelines that add an organization-scoped API endpoint or business row.

Proposed: awaiting operator approval.

One operator is an organization with one member; a team has several. Auth owns that membership. Every business row belongs to an organization directly or through its parent. Every request names one organization, and API checks the caller against it before reading or writing business records, as infrastructure D2 requires. A resource from another organization appears not found.

The API path is `/v1/orgs/{organization_id}/...`. A later endpoint violates this decision if it takes the ownership scope from a body field or from the resource being accessed instead of the request's authorized organization.

What would overturn this: a need for several brands under one bill or one set of admins, requiring another level above organizations.

## Next review units

These are scopes for new draft intents, not decisions or approval of the removed proposals:

1. [Scheduling and recovery](../00003-scheduling-recovery/intent.md): pinned times, queues, deadlines, and re-drive after Restate loss.
2. [Provider delivery](../00004-provider-delivery/intent.md): Mastodon and Threads identity, credentials, and verified commit behavior.
3. [Media and channel limits](../00005-media-capabilities/intent.md): stable media, validation, and provider processing.
4. [Team publishing policy](../00006-team-review/intent.md): roles, service actors, and approval before scheduling.

Each unit gets its own intent and spec only when work begins. Its decisions may refine this contract, but an approved change to D1, D4, D5, or D6 needs an explicit `Overturns:` decision and operator approval.
