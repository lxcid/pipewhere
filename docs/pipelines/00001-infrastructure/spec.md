# Spec 00001 — Deployment topology and service boundaries

## Context

The intent fixes three infrastructure components and four application services. That is seven running processes in the default topology. Their resource footprint matters, but it does not erase their operational cost. Each process still has to be started, upgraded, observed, and debugged.

This spec records why the separation is worth that cost and where state belongs. A later reader should be able to reverse one choice without rewriting the problem in the intent.

## Decisions

### D1 — The default topology has three infrastructure components and four application services

    postgres  business records
    rustfs    stored object bytes
    restate   durable execution

    web       Next.js frontend
    auth      Hono and Better Auth
    api       public Rust API
    worker    Rust Restate handlers

The four application services have different deployment boundaries:

- Web must be deployable to edge infrastructure.
- Auth owns the identity protocol and token boundary.
- API is the common product interface for every client.
- Worker runs scheduled and long-running work without coupling its lifecycle to request handling.

This costs more operationally than combining services into fewer processes. Small binaries reduce memory and CPU use, but they do not reduce the number of things an operator must run. That trade-off is accepted because the four boundaries are product requirements rather than speculative decomposition.

What would overturn this: the complete stack cannot remain responsive on an entry-level MacBook Pro, or operating seven processes proves materially harder than the independent deployment boundaries are worth. API and Worker are the first consolidation candidate because both are Rust services and can share an application layer without changing client contracts.

### D2 — Auth issues access tokens and owns organizations; API authorizes business resources

Binding: every pipeline that adds an organization-scoped API action, a table whose rows belong to an organization, or a foreign key into `auth`.

Auth is a Hono service built with Better Auth. It owns identities, interactive sessions, API keys, and access-token issuance. Through Better Auth's organization plugin, it also owns organizations, their members, members' roles, and invitations. An organization is what the intent calls a workspace. Its tables live in an `auth` schema in PostgreSQL. Auth owns and runs migrations for that schema. Pipewhere does not build an authentication system of its own.

Every id Auth generates follows D6.

API's tables live in a `pipewhere` schema, and API owns and runs its migrations. A privileged setup step creates both schemas and both database roles before either service migrates.

The operator chose to have tables in `pipewhere` hold foreign keys into `auth`, so the database itself refuses a row for an organization that does not exist. Auth's migrations therefore run first. API's migrations fail if Auth's have not run.

Auth grants API's database role only what it needs, column by column:

- read access to organizations, to members with their role, and to API keys with their organization, permissions, enabled flag, and expiry
- references to organization and API key ids, so that `pipewhere` tables can hold foreign keys to them

API can read no other column. Better Auth keeps session tokens, password hashes, and sign-in providers' tokens in the `auth` schema, and API never sees them. A column a Better Auth upgrade adds stays hidden from API until Auth grants it.

API never writes to `auth`. Every change to an organization, member, role, invitation, or key goes through Auth, so Better Auth's own rules apply to it.

The limit holds in both directions. Auth and API connect as separate database roles, and Auth's role has no access to the `pipewhere` schema. Worker connects as API's role. It runs the same application layer against the same tables, per D4, so a role of its own would need the same grants and would separate nothing. A leaked credential reads only what its services need.

API reads Better Auth's tables as Better Auth defines them, so upgrading Better Auth can change what API reads. API's tests run against a database with Auth's migrations applied. They rerun whenever Auth's migrations or Better Auth's version change, so an upgrade that changes a column API reads fails them before it ships.

A foreign key also changes what Auth can delete:

- A foreign key to an organization never cascades. It refuses to delete the organization while rows in `pipewhere` reference it. With organization deletion off, this is a backstop that fails loudly instead of removing or orphaning business records.
- Better Auth deletes API keys on request, when they expire, and when a usage-limited key runs out. Only a row that exists solely for one key holds a foreign key to it, and that foreign key cascades. A row that records which key acted keeps the id with no foreign key, so Better Auth's cleanup never deletes a business record.

Auth exchanges a session or an API key for a signed JWT access token that expires within 15 minutes. It publishes the verification keys as JWKS. API validates the signature, issuer, audience, and expiry locally, then reads `sub`. API does not call Auth on each request.

- **Sessions.** A session token carries its user's id in `sub`.
- **API keys.** An API key is owned by one organization, not by the member who created it. Auth configures Better Auth's API key plugin with `references: "organization"`, so no key is owned by a user. Removing the member who created a key leaves it working. Its token carries the key's id in `sub`.

The prefix of `sub` says which kind of token it is: `usr_` for a session, `key_` for an API key.

A token carries only `sub` and the standard claims: issuer, audience, issue time, and expiry. It holds no organization, role, or permission, so API always reads those from Auth's tables.

Nothing records which member created a key. When a member who could manage keys leaves, the members who still can review the organization's keys.

Better Auth's organization plugin keeps an active organization on each session. Pipewhere does not use it. Each request names the organization it acts in. The operator chose this so that:

- A request means the same thing whichever organization its user last opened.
- One user can work in two organizations at once.

Better Auth's organization endpoints fall back to the active organization when a call names none. Better Auth also sets it itself, when a user creates or joins an organization. Auth therefore keeps every session's active organization empty, through a session database hook. A call that names no organization then never acts on the organization the user last created or joined. The member, invitation, and organization changes clients use fail with `NO_ACTIVE_ORGANIZATION`. Some reads, such as getting an organization, return nothing instead.

Joining an organization needs a verified email. The operator chose this over letting an unverified invitee join. Auth sets Better Auth's `requireEmailVerificationOnInvitation` to `true` explicitly:

- D6's id generator would otherwise switch it on silently.
- D6 puts invitation ids in logs and URLs, where they are not secrets. A verified email is what proves the invitee owns the invited address.

A team deployment therefore needs email delivery, or a sign-in provider that reports the email as verified, before a teammate can join. A single operator needs neither.

Organization deletion is off. Auth sets Better Auth's `disableOrganizationDeletion`:

- Better Auth's delete removes an organization's members and invitations, but not its API keys. A key's owner is a plain reference Better Auth does not constrain, so the keys would be stranded outside any organization.
- The organization's rows in `pipewhere` would also have to go, or block the delete.

Deleting an organization waits for a pipeline that needs it. That pipeline defines one lifecycle covering business rows and keys together. A cleanup hook alone could leave a partial delete when a later step fails.

API owns resource-level authorization. On every request it checks, in Auth's tables, that the caller may act in the organization the request names:

- For a session: the user is a member of that organization, and their current role permits the action.
- For an API key: the key still exists, is enabled, has not expired, and is owned by that organization. Its current permissions permit the action.

A request that fails this check gets one of three responses, checked in this order:

- **Unauthorized:** the token's subject no longer authenticates. `sub` is missing or has neither prefix, or the key is deleted, disabled, or expired.
- **Not found:** the caller cannot act in the organization at all. The user is not a member, or another organization owns the key. An organization that does not exist gets the same response.
- **Forbidden:** the caller can act in the organization, but its role or the key's permissions do not allow the action.

A forbidden response reveals nothing about an organization the caller does not belong to, because membership is checked first. An unauthorized response tells a key's holder only what the token exchange already would.

Removing a member, changing a role, revoking a key, or changing a key's permissions therefore takes effect on the next request, even while a token is still valid.

Revoking a session prevents new tokens, but an issued session token stays valid until it expires. The membership check still applies to it.

Auth determines identity, organization membership, roles, and key permissions. API decides what those permit on a resource. Auth does not query product resources.

Web does not become a privileged backend. Web, REST, MCP, and CLI clients all obtain the same access-token shape and call the same API.

A later pipeline is in violation if it adds:

- an organization-scoped action that reads or writes before API checks the caller against the organization the request names
- a table whose rows belong to an organization directly, without a foreign key from its organization id to that organization
- a cascading foreign key to an organization
- a foreign key to an API key from a row that must outlive the key

Before the auth boundary is considered verified, Pipewhere must exercise:

- session-to-token exchange, with the user's `usr_` id in `sub`
- API-key-to-token exchange, with the key's id in `sub`
- a token whose `sub` is missing or has neither prefix, and a disabled or expired key, each reported as unauthorized
- a request naming an organization the user is not a member of, or one that does not own the key, reported as not found
- a member whose role does not permit an action, and a key whose permissions do not, each reported as forbidden
- after a user creates an organization or accepts an invitation, a call to Better Auth that names no organization never acting on either: each organization change clients use fails, and a read returns nothing
- an invitee whose email is not verified being refused when accepting an invitation
- deleting an organization through Auth being refused
- removing a member, changing a role, revoking a key, and changing a key's permissions while a token is still valid, each taking effect on the next request
- API's database role reading only the columns Auth grants it, and being refused on the rest
- Auth's database role being refused on the `pipewhere` schema
- API verification across a signing-key rotation

What would overturn this:

- Better Auth cannot issue the required claims, or local verification proves unreliable. The fallback is token introspection through Auth, accepting the per-request network dependency explicitly.
- The per-request check proves too slow. The fallback is a membership cache with a stated maximum age, accepting that delay explicitly.
- The 15-minute window after a session is revoked proves unacceptable. The fallback is a session-id claim that API checks against Auth's session table. API would be granted each session's id and expiry, not its token.
- Better Auth upgrades change the columns API reads often enough that fixing API each time costs more than a stable layer. The fallback is read-only views for reads, with the foreign keys kept on the tables.

### D3 — Web is an independent, edge-deployable Next.js service

Web runs separately from API. It may use runtime-backed Next.js features, including server rendering and route handlers, when the selected edge platform supports them. Static export remains an implementation option, not an architectural constraint.

The product requires an independently deployable web service. Removing its runtime is therefore not the service-count optimization that drives the topology.

What would overturn this: the edge target cannot support a required Next.js feature, or operating a separate web runtime provides no measurable product or deployment benefit.

### D4 — Postgres owns business records; Restate holds execution state only

Binding: every pipeline that adds a workflow.

PostgreSQL is authoritative for business records, including object references. RustFS is authoritative for stored object bytes. Both stores need backups. They are not updated or backed up atomically, so a committed record can reference missing bytes. Reading that reference returns a terminal missing-object error; it never silently treats the object as absent media.

Restate's journal and keyed state hold execution state only, including in-flight invocations, timers, and serialization. Reads of business records never require Restate.

Worker handlers may call the same application layer as API, but Restate does not become an alternative write path or an authoritative read model. A later pipeline is in violation if losing Restate loses business records or object bytes, if reading business records requires querying Restate, or if a workflow writes data outside the shared application boundary.

This keeps ownership unambiguous and makes the execution layer replaceable. It costs additional PostgreSQL reads when a handler needs state across steps, which is accepted.

Losing Restate's volume loses in-flight invocations and pending timers. Business records and object bytes survive in PostgreSQL and RustFS, but scheduled work does not run again until it is re-driven from PostgreSQL. The first pipeline that adds scheduled work owns that recovery path.

A re-drive must be safe even if an original invocation is still live. Recoverable workflows claim state transitions with conditional updates in PostgreSQL; a duplicate invocation that loses a claim stops. This protects the database transition, not an external effect that Restate may replay after the effect succeeds but before its result is durable. The pipeline adding such an effect must define and test its replay behavior, including failure after external acceptance and before Restate records the result, before shipping the workflow.

What would overturn this: a measured read path where the PostgreSQL round trip is too slow and Restate's keyed state is the natural authoritative owner. A cache does not overturn the decision because losing a cache affects performance, not authoritative business records or object bytes.

### D5 — RustFS is the default S3-compatible object store

The operator selected RustFS because it provides a broad S3-compatible surface and supports both single-node and distributed deployments. That fits Pipewhere's path from a local Compose instance to a hosted deployment without changing object stores.

RustFS is young. Version 1.0 was released in September 2026, so feature claims are not enough evidence on their own. Before the object-store boundary is considered verified, Pipewhere must exercise:

- Object create, read, delete, and list; multipart upload; presigned upload and download; and browser CORS.
- Any Restate snapshot write and restore path.
- An in-place upgrade that preserves stored objects.
- Moving stored objects from a single-node instance into the distributed topology.
- Restoring PostgreSQL and RustFS backups into a fresh deployment, reading an object referenced by a restored business record, and confirming that a missing referenced object produces a terminal error.

What would overturn this: those contract checks fail, upgrades cannot preserve stored data safely, or the single-node deployment cannot move to the hosted topology without an operator-visible migration burden greater than using another S3-compatible store.

### D6 — Every id is a short prefix and a UUIDv7

Binding: every pipeline that adds a table with its own id, including Auth's tables.

Every generated id has the form `prefix_suffix`, for example `usr_01m36mep00e3ca12pwd1gkjwex`:

- The prefix is 3 to 5 lowercase letters, and names the entity.
- The suffix is a UUIDv7, written as 26 characters of Crockford base32, in lowercase.
- An id is therefore at most 32 characters.

This is the TypeID format.

Ids are stored as text. A table keyed by its parent has no id of its own.

- **Pipewhere's tables** check the prefix and the format with a check constraint.
- **Better Auth's tables** get no check constraint, so Auth never alters a table Better Auth migrates. Better Auth generates their ids through Auth's id generator, which is told the table it is generating for. The generator throws for a table with no listed prefix. A Better Auth plugin that adds a table fails loudly until its prefix is listed.

Each pipeline lists the prefixes it adds in a table in its own spec, written with the trailing underscore. A prefix is never reused. The full list is derived, never maintained. This prints any prefix claimed twice, and nothing when every prefix is unique:

```bash
grep -ho '^| `[a-z]\{3,5\}_`' docs/pipelines/*/spec.md | sort | uniq -d
```

Auth's prefixes:

| Prefix | Entity                          |
| ------ | ------------------------------- |
| `usr_` | user                            |
| `ses_` | session                         |
| `acc_` | sign-in method linked to a user |
| `ver_` | verification token              |
| `org_` | organization                    |
| `mem_` | member                          |
| `inv_` | invitation                      |
| `key_` | API key                         |
| `jwk_` | signing key                     |

The operator chose the prefix and the 32-character limit. The prefix makes an id readable in logs, URLs, and events. It also lets one field hold ids of different kinds, such as a token's `sub`, which holds a user id or an API key id. Base32 keeps the id within 32 characters, where a hex UUID alone would already take 32. A UUIDv7 sorts by creation time to the millisecond, so new rows land at the end of their index. Within one millisecond, order is not guaranteed. It is also a standard UUID, so under the fallback below each id decodes to the same UUID, and no id is reassigned.

A later pipeline is in violation if it adds a table whose id is a bare UUID or an integer sequence, uses a prefix it does not list, or reuses one.

What would overturn this: text ids costing measurable storage or index time. The fallback is a native `uuid` column for Pipewhere's own ids, with the prefix added at the API boundary. Columns that reference Auth's ids stay text. Auth's ids keep their prefix, which D2's check of `sub` relies on.

## Implementation constraints and evidence

Verified on 2026-09-22:

| Component        | Version observed | Standing                                    |
| ---------------- | ---------------- | ------------------------------------------- |
| PostgreSQL image | 18.6             | Started and queried on 2026-09-22           |
| Restate server   | 1.7.10           | Health and admin APIs queried on 2026-09-22 |

- PostgreSQL 18 stores data in a major-version subdirectory. Its volume mounts at `/var/lib/postgresql`, not `/var/lib/postgresql/data`, so a future `pg_upgrade --link` does not cross a mount boundary.

Run against a throwaway PostgreSQL 18 container on 2026-09-23, for D2:

- A role granted some columns of a table reads them, and is refused every other column and `SELECT *`. A column added later stays hidden from it.
- A foreign key cannot point at a view.
- A foreign key needs only `REFERENCES` on the referenced id. The referencing role still cannot read the table.
- A plain foreign key refuses the referenced row's delete. A cascading one lets Auth's role delete it and removes the referencing row, although Auth's role has no rights on `pipewhere`.

Read in Better Auth's source at commit `3d0efa3`, dated 2026-09-22, for D2. Auth pins no Better Auth version yet, so the build rechecks these against the version it pins:

- Better Auth builds a session only from an API key a user owns. For a key an organization owns, it fails with `INVALID_REFERENCE_ID_FROM_API_KEY`. Auth's key-to-token exchange is therefore its own endpoint, which verifies the key and signs the token.
- Better Auth accepts a key's permissions only from server code. Auth sets them in its own endpoint.
- Better Auth's key rate limits and usage counts apply when a key is verified. They count token exchanges, not API requests, so one exchange buys up to 15 minutes of requests.
- Better Auth's JWT plugin copies the whole user record into the claims unless the payload is defined. Auth defines it, so a token carries only `sub` and the standard claims.
- Better Auth deletes a key on request, when it expires, and when a usage-limited key has no uses left and no refill.
- Better Auth's organization delete removes the organization's members and invitations, and not its API keys. A key's owner is a plain `referenceId`, with no foreign key. `disableOrganizationDeletion` refuses the delete.
- Better Auth sets a session's active organization when its user creates an organization or accepts an invitation. It writes it through its session update, which runs database hooks, and a hook can replace the value written.
- Better Auth's organization endpoints fall back to the active organization when a call names none. With neither, its changes fail with `NO_ACTIVE_ORGANIZATION`, and some reads, such as getting an organization, return `null` with status 200.
- Better Auth calls the configured id generator with the table's name, for every table's id. This is what D6's generator relies on.
- Better Auth requires a verified email to accept, reject, or read an invitation by id when `requireEmailVerificationOnInvitation` is unset and the id generator is a custom function. It treats such ids as possibly predictable.

Still to verify: whether registering the same deployment URI twice is idempotent in Restate 1.7.10. The build must reproduce this before choosing an automatic registration design.

Restate server and Rust SDK versions must be pinned independently. The server can be stable while the pre-1.0 SDK changes its Rust API.

These constraints do not settle boot order, port assignment, or snapshot configuration. The build decides those details for the seven-process topology.
