# Plan 00001 — Infrastructure foundation

Verified items name the command that proved them. Everything else is asserted and should be read as untested.

## Done

### Dependency stack — `docker/compose.yaml`

Postgres 18, Restate 1.7.10, and MinIO, on a contiguous `53xx` host port block, each bound to the loopback interface, each with a healthcheck.

Verified with `docker compose -f docker/compose.yaml up -d --wait`, all three reaching `healthy`, then:

| Check                                               | Result                          |
| --------------------------------------------------- | ------------------------------- |
| `psql -h 127.0.0.1 -p 5310 … -c "select version()"` | PostgreSQL 18.6                 |
| `psql … -c "show data_directory"`                   | `/var/lib/postgresql/18/docker` |
| `curl http://127.0.0.1:5321/health`                 | 200                             |
| `curl http://127.0.0.1:5321/deployments`            | `{"deployments":[]}`            |
| `curl http://127.0.0.1:5321/version`                | `1.7.10`, admin API 2–4         |
| `curl http://127.0.0.1:5320/restate/health`         | 200                             |
| `curl http://127.0.0.1:5330/minio/health/live`      | 200                             |

Two corrections during the build, both recorded as decisions in the spec:

- Postgres 18 refuses a volume mounted at `/var/lib/postgresql/data`. The mount moved one level up so that major-version upgrades stay possible. (Spec D9.)
- MinIO's Docker Hub repository is no longer pullable. Pinned to `quay.io/minio/minio:RELEASE.2025-09-07T16-13-09Z` from quay.io.

### Monorepo toolchain — `.prototools`, `.moon/`, `.oxfmtrc.json`

moon 2.2.4 manages the workspace; proto 0.62.0 pins tool versions. `.moon/workspace.yml` declares `apps/*` and `crates/*` as project globs. `.moon/toolchains.yml` registers the node and pnpm toolchains, which inherit their versions from `.prototools` rather than restating them.

Pinned: Node 26.10.0, pnpm 11.27.1. Both installed and active, verified with `proto install` followed by `node --version` and `pnpm --version`.

oxfmt 0.68.0 formats the repository, configured in `.oxfmtrc.json` with `proseWrap: "never"` scoped to Markdown through an `overrides` entry. Verified with `pnpm fmt` followed by `pnpm fmt:check` reporting all 15 files correctly formatted, which also confirms the formatting is idempotent — oxfmt has a known non-idempotency bug with lists containing fenced code blocks, and `pipelines/README.md` contains one.

Two findings worth carrying forward:

- oxfmt 0.70.0, the current release, is blocked by a `minimumReleaseAge` supply-chain policy on this machine because it was published the same week. Native bindings are optional dependencies, so the block failed silently and left an oxfmt that could not run rather than an install error. Pinning 0.68.0, published eight days earlier, satisfies the policy. Any future dependency added within a week of its release will hit the same wall, and the symptom will not name the cause.
- `pnpm install` wrote a `minimumReleaseAgeExclude` list into `pnpm-workspace.yaml` covering every platform binding except the one this machine needs. It was reverted rather than kept, since the file is meant to declare workspace packages.

Rust is deliberately not pinned yet. proto offers 1.98.1 and the machine has 1.95.0 installed through rustup. The version belongs with the Cargo workspace step below, and it is the operator's call.

## Remaining

### 1. Cargo workspace

Two crates to start: a library and `apps/server`. Crates get split when a seam appears, not in advance.

### 2. Two listeners in one process

axum on 8081 for the REST API and the static UI. `restate-sdk` `HttpServer` on 9080 for Restate's call path. Both as tokio tasks; the process exits if either fails to bind.

### 3. Configuration and secrets

`PIPEWHERE_`-prefixed environment variables, with `.env.example` as the reference. `PIPEWHERE_ENCRYPTION_KEY` is required and never generated (Spec D7). Startup fails with a message naming the variable and how to produce a value.

### 4. Migrations on boot

`sqlx::migrate!`, embedded in the binary, run before either listener accepts traffic. No migration image and no separate command (Spec D6).

For this pipeline the migration set is whatever proves the mechanism works. The real schema belongs to pipeline 00002.

### 5. Restate self-registration

POST the application's own handler endpoint to the Restate admin API at boot, with force, retrying until Restate is reachable (Spec D5). Registration is not idempotent, so force is required on every restart after a code change.

Needs one throwaway handler to register, otherwise there is nothing to discover.

### 6. Health and readiness

`/health` is liveness. `/ready` checks Postgres, the object store, and Restate registration, and fails when any is missing. Compose uses `/ready` as the app healthcheck.

### 7. Object store bucket initialisation

An init step that creates the media bucket and fails loudly if it cannot. Pending the MinIO-or-Garage decision in the intent, since the init sequence differs.

### 8. Application container and compose integration

A Dockerfile producing the server binary with the static UI embedded, added to compose with `depends_on` on the healthy dependencies.

### 9. Next.js static export

Blocked on the intent's open question. If the answer is a Node runtime instead, this step becomes a fifth service and Spec D2 is rewritten.

### 10. Quickstart

The README path from clone to running instance. The outcome in the intent is only met when someone who has not seen this repository can follow it.

## Not in this pipeline

- The real database schema. That is pipeline 00002.
- The reconciliation sweeper. It is specified in D4 as the reason Restate's volume is rebuildable, but it needs publications to sweep.
- Clustering, Restate snapshots, and rolling upgrades with versioned endpoints. Named in the spec so they are not surprises; not built.
