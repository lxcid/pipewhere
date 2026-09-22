# Spec 00001 — Deployment topology and self-hosting

Status: draft

## Context

Self-hosting is a primary product constraint. Every service in the topology is something an operator has to run, expose, back up, upgrade, and debug at 2am. The count of services is therefore a product metric, not an implementation detail.

The stack is fixed: Rust, Next.js, Restate, Postgres, S3-compatible storage.

Versions verified 2026-09-22:

| Component            | Version                                                 |
| -------------------- | ------------------------------------------------------- |
| Restate server       | 1.7.10 (`docker.restate.dev/restatedev/restate:1.7.10`) |
| `restate-sdk` (Rust) | 0.12.1                                                  |
| Postgres             | 18                                                      |
| Rust toolchain       | 1.95                                                    |
| Node / pnpm          | 22.22 / 11                                              |

## Decisions

### D1 — Four services in the default topology

    pipewhere   single Rust binary: REST API, static UI, Restate handlers
    postgres    source of truth
    restate     execution
    object store  media, and Restate snapshots if clustered

No Node runtime. No reverse proxy. No Valkey. Anything added to this list has to justify itself against the operator cost of a fifth thing to run.

### D2 — The web UI is a static export served by the Rust binary

Next.js builds with `output: 'export'`. The Rust binary serves the resulting files.

This removes a Node process, a container, and a reverse proxy from every self-hosted install. It also makes "the web client contains no unique business behavior" structural rather than a convention: there is no server for business logic to accumulate in.

OAuth callbacks land on the Rust API, which handles the exchange and redirects to the static UI. No Node is required for account connection.

Cost: no SSR, no server components requiring a runtime, no route handlers, no server actions. The UI is a client of the public API like every other client.

Reversible. If SSR becomes necessary the UI moves to its own container and the API does not change.

### D3 — One object store serves media and Restate snapshots

MinIO locally, any S3-compatible service in production. Separate prefixes.

Restate's snapshot destination is S3-compatible, and media already requires object storage, so the second use is free.

Media access always goes through `GET /media/:id` on the Rust API, which redirects to a short-lived presigned URL on an S3 backend or streams bytes on a filesystem backend. One API shape, two backends. This keeps a single-box install with no object store viable; such an install forgoes Restate snapshots, which only matter for clusters.

Buckets are never public. The media lifecycle is its own pipeline, not yet opened.

### D4 — Postgres is the source of truth; Restate holds execution state only

Binding: every pipeline that adds a workflow.

Restate is a second stateful store sitting next to Postgres. Without an explicit rule about which one owns what, the two mix by default: a workflow accumulates keyed state that a query later needs, reads start requiring an invocation, and the execution layer stops being replaceable.

Postgres holds all business state. Restate's journal and keyed state hold execution state only: in-flight invocations, pending timers, and per-account serialization. Workflows never write business state directly. They sequence calls into the application layer, and the application layer writes Postgres.

A later pipeline is in violation if reading business state requires querying Restate, or if a workflow writes a row the application layer did not.

What this buys:

- Every read stays plain SQL. `GET /publications` does not consult Restate.
- Restate is replaceable. Swapping the execution layer rewrites workflow code rather than migrating data.
- Losing `/restate-data` loses the pending schedule, not the data. A reconciliation sweeper re-invokes workflows for publications in state `scheduled` with no live invocation, and the schedule is restored. With the invocation idempotency key set to the publication id, a double invocation is harmless.

That last point is the operational payoff and the reason the rule is worth its cost: it turns the second stateful store from a backup obligation into a recoverable incident. `/restate-data` still needs to be a persistent volume, but losing it is not data loss, and the self-hosting documentation should say so in those words.

The sweeper reverses an earlier judgment. It was rejected as redundant with the transactional outbox, which was right on correctness grounds. It earns its place on disaster-recovery grounds.

What it costs:

- State a workflow needs across steps is passed through arguments or re-read from Postgres. That is redundant reads, accepted deliberately.
- The per-account virtual object's serialization guarantee is execution state and does not survive volume loss. After a restore, publication ordering for an account is whatever the sweeper produces, not the original order. This gap is tolerated; the alternative is business state in Restate, which is the thing the rule exists to prevent.

What would overturn this: a read path where the Postgres round-trip is measurably too slow and Restate's keyed state is the natural place for a hot copy. Note that a cache is not ownership. The rule would need rewriting only if the cached value became authoritative — if losing it meant losing business state rather than losing a performance property.

### D5 — The server self-registers with Restate on boot

Deployment registration through the Restate admin API is not idempotent. Re-registering the same URI requires an explicit force.

On startup the server registers its own Restate endpoint with the admin API, with force enabled, retrying until Restate is reachable. An operator never runs `restate deployments register`, and never installs the Restate CLI.

Trade-off: force-registration on restart means invocations in flight may resume against new handler code. Acceptable at one replica, which is the default topology. Rolling upgrades want versioned endpoints so old invocations drain against old code. Named here so it is not discovered during the first upgrade.

### D6 — Migrations are embedded and run on boot

`sqlx::migrate!` compiles migrations into the binary. There is no migration image, no separate command, and no step in the self-host quickstart. The sqlx migrator takes a Postgres advisory lock, so concurrent replicas are safe.

### D7 — The encryption key is required, never generated

`PIPEWHERE_ENCRYPTION_KEY` must be present at boot or the process exits with a message naming the variable and how to generate a value.

Generating one when absent would mean a container restart silently orphans every stored provider credential, and the failure would surface later as unexplained authentication errors. Fail loud at boot instead.

### D8 — Two listeners, one process

| Container port | Bound by | Purpose | Local host port | Exposure |
| --- | --- | --- | --- | --- |
| 8081 | pipewhere (axum) | REST API and static UI | 5340 | public |
| 9080 | pipewhere (restate-sdk) | Restate's call path into handlers | 5350 | internal only |
| 8080 | restate | ingress; the app calls this to start invocations | 5320 | internal only |
| 9070 | restate | admin; used for self-registration | 5321 | internal only |
| 5122 | restate | node-to-node | not mapped | cluster only |
| 5432 | postgres | database | 5310 | internal only |
| 9000 | object store | S3 API | 5330 | internal only |
| 9001 | object store | console | 5331 | internal only |

Port 9080 must never be exposed publicly. It accepts invocations from Restate and is not authenticated the way the public API is.

Container-internal ports stay standard. Host ports use a contiguous 53xx block rather than the services' defaults, because a developer machine running more than one project cannot give every stack port 5432. The sibling mempipe stack already occupies 47xx to 49xx, and unrelated containers hold 5432 and 9000.

In local compose, every port binds to `127.0.0.1` so a laptop on a shared network does not publish a database.

### D9 — The Postgres volume mounts at `/var/lib/postgresql`, not `/data`

Postgres 18 images refuse to start when the volume is mounted at `/var/lib/postgresql/data`, which is the convention most compose files copy. From 18 onward, data lives in a major-version subdirectory (verified: `/var/lib/postgresql/18/docker`) so `pg_upgrade --link` works without crossing a mount point boundary.

Mounting one level up is what makes a future major-version upgrade a normal operation instead of a dump-and-restore. For a project where operators run the database themselves for years, that is worth getting right before the first install exists.

## What an operator backs up

| Store | Contains | If lost |
| --- | --- | --- |
| Postgres | all business state and event history | data loss |
| Object store | media bytes | media loss; rows survive with broken references |
| `/restate-data` | in-flight invocations, pending timers | schedule rebuilt by the sweeper |

## Open questions

- `restate-sdk` is 0.12.1 while the server is 1.7.10. The wire protocol is stable; the Rust API surface is pre-1.0 and may churn. Pin the exact version and treat an SDK upgrade as its own change.
- Whether the SDK's ingress client exposes an idempotency key on invocation, which D4's sweeper safety depends on. Verify before relying on it.
- Whether a single-node deployment gains anything from snapshots, or whether the persistent volume is the whole story.
- The object store image. MinIO's Docker Hub repository is no longer pullable, and the newest community release on quay.io is `RELEASE.2025-09-07`, roughly a year old. Compose pins that tag and it works, but shipping a frozen dependency as the default self-host object store is a slow-burning risk. Garage is the leading alternative: actively maintained, S3-compatible, written in Rust, and built for self-hosting. It costs a longer init sequence than MinIO's zero-config start. The choice is not architectural, since the application speaks the S3 API either way, but it is the default an operator inherits.
