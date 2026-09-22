# Spec 00001 — Deployment topology and service boundaries

## Context

The intent fixes three infrastructure components and four application services. That is seven running processes in the default topology. Their resource footprint matters, but it does not erase their operational cost. Each process still has to be started, upgraded, observed, and debugged.

This spec records why the separation is worth that cost and where state belongs. A later reader should be able to reverse one choice without rewriting the problem in the intent.

## Decisions

### D1 — The default topology has three infrastructure components and four application services

    postgres  business state
    rustfs    object storage
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

API owns resource-level authorization. It maps the principal identifier to workspaces in the business schema and decides whether the represented scopes permit the requested action on that resource. Auth determines identity and which coarse scopes a credential may request; it does not query product resources.

Web does not become a privileged backend. Web, REST, MCP, and CLI clients all obtain the same access-token shape and call the same API. Revoking a session or long-lived credential prevents new tokens, but an issued token remains valid until it expires. That bounded delay is accepted in exchange for removing an Auth network hop from every API request.

What would overturn this: Better Auth cannot issue the required claims, local verification proves unreliable, or a 15-minute revocation delay is unacceptable. The fallback is token introspection through Auth, accepting the per-request network dependency explicitly.

### D3 — Web is an independent, edge-deployable Next.js service

Web runs separately from API. It may use runtime-backed Next.js features, including server rendering and route handlers, when the selected edge platform supports them. Static export remains an implementation option, not an architectural constraint.

The product requires an independently deployable web service. Removing its runtime is therefore not the service-count optimization that drives the topology.

What would overturn this: the edge target cannot support a required Next.js feature, or operating a separate web runtime provides no measurable product or deployment benefit.

### D4 — Postgres is the source of truth; Restate holds execution state only

Binding: every pipeline that adds a workflow.

PostgreSQL holds all business state. Restate's journal and keyed state hold execution state only, including in-flight invocations, timers, and serialization. Reads of business state never require Restate.

Worker handlers may call the same application layer as API, but Restate does not become an alternative write path or an authoritative read model. A later pipeline is in violation if losing Restate loses business state, if reading business state requires querying Restate, or if a workflow writes data outside the shared application boundary.

This keeps ownership unambiguous and makes the execution layer replaceable. It costs additional PostgreSQL reads when a handler needs state across steps, which is accepted.

Losing Restate's volume loses in-flight invocations and pending timers. Business state survives, but scheduled work does not run again until it is re-driven from PostgreSQL. The first pipeline that adds scheduled work owns that recovery path.

What would overturn this: a measured read path where the PostgreSQL round trip is too slow and Restate's keyed state is the natural authoritative owner. A cache does not overturn the decision because losing a cache loses performance, not business state.

### D5 — RustFS is the default S3-compatible object store

MinIO's community repository is archived and no longer maintained. The operator selected RustFS as the replacement because it keeps a MinIO-shaped operating model and a broad S3 surface while supporting both single-node and distributed deployments. Garage also appears to satisfy Pipewhere's current object operations; features beyond that contract are not part of the justification for RustFS.

RustFS is young. Version 1.0 was released in September 2026, so feature claims are not enough evidence on their own. Before the object-store boundary is considered verified, Pipewhere must exercise the operations it relies on: object create, read, delete, and list; multipart upload; presigned upload and download; browser CORS; and any Restate snapshot write and restore path.

What would overturn this: those contract checks fail, upgrades cannot preserve stored data safely, or the single-node deployment cannot move to the hosted topology without an operator-visible migration burden greater than using another S3-compatible store.

## Verified implementation constraints

Established on 2026-09-22:

| Component        | Version observed | Standing                                             |
| ---------------- | ---------------- | ---------------------------------------------------- |
| PostgreSQL image | 18.6             | Started and queried on 2026-09-22                    |
| Restate server   | 1.7.10           | Health and admin APIs queried on 2026-09-22          |
| Restate Rust SDK | 0.12.1           | Evaluated against server 1.7.10; not a permanent pin |

- PostgreSQL 18 stores data in a major-version subdirectory. Its volume mounts at `/var/lib/postgresql`, not `/var/lib/postgresql/data`, so a future `pg_upgrade --link` does not cross a mount boundary.
- Registering the same Restate deployment URI again is not idempotent. Any automatic registration design must handle replacement explicitly rather than treating a repeated request as a harmless no-op.
- Restate server and Rust SDK versions must be pinned independently. The server can be stable while the pre-1.0 SDK changes its Rust API.

These constraints do not settle boot order, port assignment, or snapshot configuration. The build decides those details for the seven-process topology.
