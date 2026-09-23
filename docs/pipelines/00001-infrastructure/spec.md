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

### D2 — Auth issues access tokens; API authorizes business resources

Auth is a Hono service built with Better Auth. It owns identities, interactive sessions, long-lived client credentials, and access-token issuance. Its tables live in an `auth` schema in PostgreSQL. Auth owns and runs migrations for that schema; API never reads it.

Auth exchanges a session or long-lived credential for a signed JWT access token that expires within 15 minutes. It publishes the verification keys as JWKS. API validates the signature, issuer, audience, and expiry locally, then reads the opaque principal identifier and coarse scopes from the claims. API does not call Auth on each request.

An interactive session carries its user's subject in both `sub` and `owner_sub`. Each long-lived client credential is a separate principal and carries its own subject. Its creator remains its owner, and Auth includes that user's subject as an immutable `owner_sub` claim in the credential's access token. If Better Auth cannot model that relationship directly, Auth represents the credential as a service identity linked to its creator. Users and client credentials gain workspace access through the same business-schema membership rows.

API owns resource-level authorization. It requires active workspace membership for both `sub` and `owner_sub` on every request and rejects tokens missing either claim. It decides whether the represented scopes permit the requested action on that resource. Auth determines identity and which coarse scopes a credential may request; it does not query product resources.

Web does not become a privileged backend. Web, REST, MCP, and CLI clients all obtain the same access-token shape and call the same API. Revoking a session or long-lived credential in Auth prevents new tokens, but an issued token remains valid until it expires. Removing either the credential's or its owner's workspace membership blocks its next API request because API checks both every time. The bounded token lifetime is accepted in exchange for removing an Auth network hop from every request.

Before the auth boundary is considered verified, Pipewhere must exercise session-to-token exchange with equal `sub` and `owner_sub` claims, credential-to-token exchange with distinct claims, rejection of tokens missing either claim, removal of either membership while a token is still valid, and API verification across a signing-key rotation.

What would overturn this: Better Auth cannot issue the required claims, local verification proves unreliable, or a 15-minute revocation delay is unacceptable. The fallback is token introspection through Auth, accepting the per-request network dependency explicitly.

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

Every generated id has the form `prefix_suffix`, for example `usr_01j8zq3k7w5d9m2x4c6v8b0n1r`:

- The prefix is 3 to 5 lowercase letters, and names the entity.
- The suffix is a UUIDv7, written as 26 characters of Crockford base32, in lowercase.
- An id is therefore at most 32 characters.

This is the TypeID format.

Ids are stored as text, with a check constraint on the prefix and the format. Better Auth already stores its ids as text. It generates them through its id generator, which is told the table it is generating for. A table keyed by its parent has no id of its own.

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

The operator chose the prefix and the 32-character limit. The prefix makes an id readable in logs, URLs, and events. It also lets one field hold ids of different kinds, such as a token's `sub`, which holds a user id or an API key id. Base32 keeps the id within 32 characters, where a hex UUID alone would already take 32. A UUIDv7 sorts by creation time to the millisecond, so new rows land at the end of their index. Within one millisecond, order is not guaranteed. It is also a standard UUID, so the fallback below needs no conversion.

A later pipeline is in violation if it adds a table whose id is a bare UUID or an integer sequence, uses a prefix it does not list, or reuses one.

What would overturn this: text ids costing measurable storage or index time. The fallback is a native `uuid` column, with the prefix added at the API boundary.

## Implementation constraints and evidence

Verified on 2026-09-22:

| Component        | Version observed | Standing                                    |
| ---------------- | ---------------- | ------------------------------------------- |
| PostgreSQL image | 18.6             | Started and queried on 2026-09-22           |
| Restate server   | 1.7.10           | Health and admin APIs queried on 2026-09-22 |

- PostgreSQL 18 stores data in a major-version subdirectory. Its volume mounts at `/var/lib/postgresql`, not `/var/lib/postgresql/data`, so a future `pg_upgrade --link` does not cross a mount boundary.

Still to verify: whether registering the same deployment URI twice is idempotent in Restate 1.7.10. The build must reproduce this before choosing an automatic registration design.

Restate server and Rust SDK versions must be pinned independently. The server can be stable while the pre-1.0 SDK changes its Rust API.

These constraints do not settle boot order, port assignment, or snapshot configuration. The build decides those details for the seven-process topology.
