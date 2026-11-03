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

Binding: later pipelines that add an API endpoint with a request body.

Every endpoint reads its JSON body through `JsonBody` in `apps/api/src/error.rs`, not axum's `Json`. A body it cannot read answers with API's error body, `{ "code": "invalid_body" }`, and the status axum's `Json` extractor chose:

- 400 when the body is not JSON.
- 413 when the body is over axum's default limit of 2 MB.
- 415 when the request does not say `application/json`.
- 422 when the JSON has the wrong shape.

axum's `Json` answers the same statuses with a plain-text message. A client would then need a second way to read API's errors. Keeping axum's statuses tells a client which of the four mistakes it made without adding a message field.

This decision covers only bodies the extractor cannot read. How an endpoint answers a readable body that breaks its own rules is that endpoint's decision, such as D10 for content.

The caller is checked against the organization before the body is read, per 00001 D2. An outsider sending a bad body therefore gets not found, not `invalid_body`.

A later endpoint violates this decision if it takes axum's `Json` as a request extractor.

What would overturn this: clients needing more than the status to fix a bad request, such as which field failed. That would add a message or field list to the error body.

### D10 — Content needs text other than whitespace, and no NUL

Content's `body` must hold at least one character that is not whitespace, and must not hold U+0000. API refuses other text with 422 `invalid_body`, on create and on edit, the same answer D9 gives a body of the wrong shape. Whitespace is Rust's definition, the Unicode `White_Space` property.

Text is all content holds, so blank content is not writing. Mastodon and Threads both refuse a blank post, so a publication made from it could never go out. API has no way to delete content yet, so a blank row would stay.

PostgreSQL's `text` cannot store U+0000. Without the check, such text fails at the insert and answers 500, as if API were at fault.

No other limit applies. Content has no channel, so provider length limits belong to publications. Request bodies are capped at 2 MB by axum's default, per D9.

The table does not check the body. PostgreSQL's whitespace classes differ from Rust's, and a check that disagrees with API would refuse text API accepted. API is the only writer, since Worker runs the same application layer.

What would overturn this: a client that saves a draft before any text is typed, such as an editor that autosaves.

### D11 — A publication request supplies its text or names content

`POST /v1/orgs/{organization_id}/publications` takes a `channel_id`, and either `body` or `content_id`:

- `body` creates content with that text, in the same transaction as the publication, per D2. The text follows D10.
- `content_id` names existing content. The publication copies its body as it is at that moment.
- A request with both, or with neither, answers 422 `invalid_body`, as D10 answers text it refuses.
- A field the request does not define answers 422 `invalid_body`. A misspelt `planned_at` would otherwise be dropped, and the publication planned for now.

A publication's body follows D10 as well: at least one character that is not whitespace, and no U+0000. D10's reasons hold for it. Mastodon and Threads refuse a blank post, and PostgreSQL text cannot store U+0000. On create, the body is always content's text, which D10 has already checked. #17's edit applies the same check. Provider length limits belong to the adapter pipelines, 00004 and 00008.

A `channel_id` or `content_id` outside the request's organization answers 422 `channel_not_found` or `content_not_found`. Another organization's channel gets the same answer as a channel that does not exist, per D6. So does an id holding U+0000, which PostgreSQL text cannot store, so no id holds one. The body's references do not answer 404, because 404 says the path's organization or publication was not found.

Create does not check the channel's status. [Scheduling and recovery D3 and D4](../00003-scheduling-recovery/spec.md) fail a publication whose channel cannot publish when its commit is claimed, not when it is created. Channels have no status until 00004.

### D12 — The planned time defaults to the time of the request

`planned_at` is an RFC 3339 timestamp with an offset. The schema publishes it as a `date-time` string, and API answers it in UTC. PostgreSQL keeps microseconds, so finer precision is dropped.

Its year in UTC must be from 0000 to 9999, the years RFC 3339 can write. API refuses other times with 422 `invalid_body`. A time written within those years can still leave them. An offset can move it across a year boundary in UTC, and a leap second at the end of 9999 carries into 10000 when stored. API checks the stored year, because it would otherwise store and answer such a time outside the published format.

When the request omits it, the planned time is the time of the request, by the database's clock. That is how a client publishes now, per D3. Making it required would tie publishing now to the client's clock.

Any time within those years is accepted, including one in the past. Lateness belongs to [scheduling and recovery D1 and D2](../00003-scheduling-recovery/spec.md): a publication whose commit cannot start by its planned time plus its tolerance fails as late. Refusing a past time here would need an allowance for clock skew, which is that tolerance under another name.

A publication does not store a tolerance yet. Scheduling and recovery D2 records one at create, from the organization's default of 30 minutes. No organization can change that default until 00003 adds the setting, so 00003's migration can give existing publications 30 minutes, which is what each would have recorded. Nothing runs a publication before 00003.

### D13 — Idempotency keys live in their own table, one key space per organization

Binding: 00006, and any pipeline that gates reading publications.

This is how create meets D5.

- **Storage.** `pipewhere.idempotency_key` holds one row per key: its organization, the key, the request hash, and the publication the request produced. Its primary key is `(organization_id, key)`, so a key is unique within an organization whichever member or API key sends it. A table of its own, rather than columns on the publication, because #17's retry also takes a key. A publication can be retried many times, each time with a new key, and both operations must share one key space.
- **The header.** `Idempotency-Key` is opaque text of 22 to 255 visible ASCII characters, the characters an HTTP header value can carry as text. API cannot measure randomness, so it checks only the length. 22 characters is the shortest base64url text that holds 128 bits. A missing key, or one outside that length, answers 400 `invalid_idempotency_key`.
- **The request hash.** SHA-256 of the parsed request: the operation's name, then `channel_id`, `body`, `content_id`, and `planned_at` in UTC, as a JSON array. Hashing the parsed request means whitespace, field order, and a timestamp's offset do not make a retry look like a different request. An omitted `planned_at` hashes as omitted, so a retry of a request to publish now matches its first request.
- **One transaction.** API inserts the key's row first, with `ON CONFLICT DO NOTHING`, then the content and the publication, and commits them together. PostgreSQL makes a second insert of the same key wait until the first transaction ends. If the first commits, the second finds its row. If it rolls back, the second goes ahead. Concurrent use of one key therefore produces one publication, without a lock of API's own. The key's reference to its publication is checked at commit, because the publication is inserted after the key.
- **Refused requests store nothing.** A request refused after its key is claimed, such as one naming an unknown channel, rolls the key back. The client can fix the request and send it with the same key.
- **The stored result.** The same key with the same hash answers 201 with the publication the key produced, as the publication is now. The same key with a different hash answers 422 `idempotency_key_reused`. API stores the publication's id, not a copy of the first response. A client retrying after a lost response therefore sees the publication's current state, and no second copy of its body is kept.
- **Authorization.** A stored result passes the same `OrgCaller` check as any request, so the caller must still be able to act in the organization. Reading a publication is open to every member and key, per infrastructure D10. A pipeline that gates reading publications must apply that gate to a stored result too.

Keys have no expiry, per D5. A key's row lasts as long as its publication.

A later pipeline is in violation if it gates reading publications and a stored result still answers with a publication the caller may not read.

What would overturn this: a stored result that answers with only the publication's id, so that it reveals nothing a read gate protects.

Tolerated: a UUIDv4 passes the length check but holds 122 random bits, fewer than D5 asks for. The header's description asks for 16 random bytes. A key with too little randomness risks colliding only with another key in the same organization, and a collision either answers 422 or returns that organization's own publication.

### D14 — The publication list pages by cursor

`GET /v1/orgs/{organization_id}/publications` answers `{ "publications": [...], "next_cursor": ... }`, newest first, 100 to a page. `next_cursor` is null on the last page. Otherwise the client sends it back as `?cursor=` for the next page. The cursor is opaque to clients.

The list needs a cursor now because it grows with every post, and adding one later changes what an existing request returns. A client written before the cursor would see only the first page, without noticing. As an estimate, a publication with a 500-character body, the default limit on Mastodon and on Threads, is about 800 bytes of JSON. At 10 publications a day, an unpaged list grows by about 3 MB a year. The channel list does without a cursor because an organization has a handful of channels. Content has no list, per #15.

- Newest first, because a growing list is read from its recent end. Order is by id, which is creation order, per infrastructure D6.
- The page size is fixed. A `limit` parameter waits for a client that needs smaller pages.
- No filter by state, channel, or time. Each waits for the first client that needs it.

### D15 — Edit, cancel, retry, and resolve are separate endpoints

Proposed: awaiting operator approval.

A person changes a publication through four operations. Each answers 200 with the publication as the change left it.

- `PATCH /v1/orgs/{organization_id}/publications/{publication_id}` edits `body`, `planned_at`, or both. An omitted field keeps its value. The body follows D10 and D11, and the time follows D12. Any other field answers 422 `invalid_body`, as on create.
- `POST .../{publication_id}/cancel` takes no body.
- `POST .../{publication_id}/retry` takes `Idempotency-Key` and a body, per D17.
- `POST .../{publication_id}/resolve` takes the outcome a person found, per D18.

A writable `state` field on the edit was rejected:

- The actions take different input. Retry needs a key and a new time, and resolve needs the remote reference. One `PATCH` would need a key for some bodies and not for others.
- D3 leaves most transitions to the worker. A writable `state` would publish values in the schema that a client may never set.
- Each action matches rows of D3, so its guard is one condition a reader can check against the table.

Tolerated:

- `planned_at: null` on an edit keeps the current time, as omitting it does. Telling the two apart needs a second deserializer, and a client that wants to publish an edited publication now sends the current time.
- Cancel and resolve take no key, since D5 requires one only for create and retry. Repeating either after a lost response answers 409 `state_conflict`, per D16. The client reads the publication to see that its first request took effect.
- The edit reads its body before looking up the publication, as content's edit does. A body D9 or D10 refuses, sent to a missing publication, answers `invalid_body`, not 404.

### D16 — A state that does not allow the action answers 409 `state_conflict`

Proposed: awaiting operator approval.

Each action is one conditional `UPDATE`. Its `WHERE` clause names the states D3 allows the action from:

| Action | From | To |
| --- | --- | --- |
| edit | `scheduled`, `failed` | unchanged |
| cancel | `scheduled`, `preparing` | `cancelled` |
| retry | `failed` | `scheduled` |
| resolve | `outcome_unknown` | `published` or `failed` |

When the update changes no row, API looks for the publication in the request's organization. If it is not there, the answer is 404 `not_found`, per D6. If it is there, the answer is 409 `state_conflict`. The error body carries only the code, and the client reads the publication for its state.

A concurrent change loses cleanly. PostgreSQL makes a second `UPDATE` of a row wait for the first transaction to end, then checks its `WHERE` clause against the committed row. A cancel racing the worker's claim of the commit therefore either cancels first or answers 409 after the claim. API takes no lock of its own. Reading the state and then writing would let a cancel overwrite `publishing`.

409 because the request is well formed and would succeed if the publication were in another state. A new code, because `invalid_body` tells the client to fix its request, which cannot help here. The code adds a value to `ErrorCode`, with D8's cost for clients that validate the enum strictly, as #16's codes did.

### D17 — A retry takes a key and a new time, and a repeat answers the publication as it is now

Proposed: awaiting operator approval.

Retry meets D5 the way create does, under D13:

- **The header.** `Idempotency-Key` follows D13's rules. Create and retry share one key space per organization.
- **The request hash.** SHA-256 of `["retry_publication", publication_id, planned_at]` as JSON, with `planned_at` in UTC. The operation's name differs from create's `create_publication`, so a key cannot pass from one operation to the other. The publication's id is part of the hash, so one key cannot retry two publications.
- **One transaction.** API claims the key's row first, with `ON CONFLICT DO NOTHING`, naming the retried publication. Then it runs D16's conditional update and commits both together. A retry refused with 404 or 409 rolls the key back, so the client can send it again with the same key.
- **The stored result.** The same key with the same hash answers 200 with the publication as it is now, and does not retry it again. If the worker has failed it again since, the answer shows `failed`, and the client retries with a new key. This follows D13's reasoning for create.

`planned_at` is optional, and defaults to the time of the request by the database's clock, as on create, per D12. [Scheduling and recovery D1](../00003-scheduling-recovery/spec.md) says a retry chooses a new planned time; omitting it chooses now. A retry keeps the publication's body. An operator who wants to fix the text edits it first, which D3 allows in `failed`. No tolerance or deadline is stored. Scheduling and recovery D2 adds them.

Tolerated: the body is required, so a retry for now sends `{}`. Every action with input then reads one JSON body through `JsonBody`, per D9.

### D18 — Resolving to `published` records the remote id, and the address when given

Proposed: awaiting operator approval.

A publication gains `remote_id` and `remote_url`. Both are null until it is `published`, and both appear in every publication response.

- `{ "outcome": "published", "remote_id": ..., "remote_url": ... }` requires `remote_id`. It must hold a character other than whitespace, and no U+0000, which PostgreSQL text cannot store. `remote_url` is optional.
- `remote_url` must be an absolute `http` or `https` URL. A client shows it as a link, so a `javascript:` address would run in that client. API stores the URL as parsed. The scheme and host are lowercased, and characters a URL cannot hold are percent-encoded, so the stored address never holds U+0000.
- `{ "outcome": "failed" }` takes neither field. Sending one answers 422 `invalid_body`.

The id is required because it is what identifies the post at its provider, and the issue names it. The address is optional because it does not identify the post, and requiring it would refuse a resolution a person can otherwise make.

The table checks `(state = 'published') = (remote_id IS NOT NULL)`. A published publication always names its post, and a remote id appears in no other state. Nothing moves a publication out of `published`, and edit is refused there, so D2's rule that its body and remote reference cannot change holds without a trigger.

Tolerated: API cannot check that the id or address names a real post on the channel's account. Under D2 a mistyped reference cannot be corrected through the API. A person resolves only after looking for the post, and a resolution is rare. Correcting a published record would need its own decision against D2.
