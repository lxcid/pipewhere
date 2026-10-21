# Spec 00002 — Publication contract

## Decisions

### D1 — One vocabulary for every interface

Binding: 00003–00006, and later API, MCP, CLI, and web pipelines that expose publications.

Proposed: awaiting operator approval.

- An **organization** is the ownership boundary, managed by Auth.
- A **channel** links one external account to one organization. It records that relationship, not the account itself. Several organizations may each link the same external account; their channels are separate.
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

Binding: 00003, 00004, 00006, and later pipelines that change a publication's state.

Proposed: awaiting operator approval.

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

What would overturn this: a provider whose commit cannot be isolated as one step after cancellable preparation, or evidence that operators need to edit a publication once preparation has started.

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

Successful keys have no time-based expiry while their publication history exists. Before returning a stored result, API checks the current caller's authorization. A client retry with a new key violates this decision because it can create a second publication. An endpoint violates it if it creates or retries a publication without an `Idempotency-Key`, or commits the key separately from the publication change. A deliberate new request with a new key may publish identical text.

What would overturn this: client-assigned publication ids that make create requests naturally idempotent.

### D6 — Organizations bound every publication request

Binding: 00003–00006 and later pipelines that add an organization-scoped API endpoint or business row.

Proposed: awaiting operator approval.

An operator working alone uses an organization with one member; a team shares one with several. Auth owns that membership. One user may belong to several organizations, such as a personal one and a company one. Each holds its own channels, content, and publications, and nothing is shared between them. Every business row belongs to an organization directly or through its parent. Every request names one organization, and API checks the caller against it before reading or writing business records, as infrastructure D2 requires. A resource from another organization appears not found.

No request reads or writes across organizations. A user who wants to see what is scheduled in both a personal and a company organization makes one request to each.

The API path is `/v1/orgs/{organization_id}/...`. A later endpoint violates this decision if it takes the ownership scope from a body field or from the resource being accessed instead of the request's authorized organization.

What would overturn this: a need to share administration, billing, or business records across organizations, requiring another level above them.
