# Plan 00001 — Self-hostable infrastructure foundation

The build runs in slices. Each slice leaves the stack running from one command.

## Slices

1. **Infrastructure and Auth, with a first Web UI.** Built.
2. **API.** Built. Local token verification, D2's per-request organization check, D9's first action, and D11's migration step. D10 defers forbidden, and D11 defers API's id generator, check constraint, and prefix test.
3. **Worker.** Built. A Restate endpoint with no handlers, registered by every full `docker compose up`, per D14. D15 makes a failed registration stop API once Worker has a handler. The operator chose to add Worker's first handler, and its database connection, with the first workflow.
4. **Footprint and README pass.** Not started. Measure the full seven-process stack, and walk the README from a fresh clone.

## Slice 1 — what was built

- `compose.yaml` runs PostgreSQL 18.6, RustFS 1.0.3, and Restate 1.7.13, then Auth and Web.
- `infra/postgres/setup.sql` is the privileged setup step. It creates the `pipewhere_auth` and `pipewhere_api` roles and the `auth` and `pipewhere` schemas, and runs again safely.
- `apps/auth` is Hono and Better Auth 1.7.12, run by Node 26 straight from TypeScript.
  - It configures the organization plugin with deletion off and verified email required on invitation, the API key plugin with organization-owned keys, and the JWT plugin with a `sub`-only payload.
  - Its id generator gives each Better Auth model its D6 prefix and throws for an unlisted model.
  - Its session hook keeps every session's active organization empty.
  - `POST /api-key/token` exchanges an organization's key for an access token.
  - `src/migrate.ts` runs Better Auth's migrations and then grants API's role its columns, as the `auth-migrate` service.
- `apps/web` is Next.js 16.4.2. It signs in and up, lists and creates organizations, shows members, creates and revokes organization keys, and shows the claims of a session token and a key token.

Choices made in the build, within the spec:

- Ports: Web on 3000 and Auth on 3001, published on localhost only. PostgreSQL, RustFS, and Restate are not published, since nothing outside Compose uses them yet.
- Boot order: `postgres` healthy, then `postgres-setup` completes, then `auth-migrate` completes, then `auth`, then `web`.
- Access tokens use issuer `AUTH_URL` and audience `pipewhere-api`.
- API's role may read only the columns `apiColumnGrants` lists in `apps/auth/src/migrate.ts`, and may reference `organization.id`.
- Auth's tests use Node's built-in test runner, against a disposable PostgreSQL server started with `pnpm --filter @pipewhere/auth test:db`.
- A refused key exchange makes Better Auth log an ERROR with a stack trace. Tolerated for now. The log comes from inside Better Auth, and its only lever is the global log level, which would also hide real errors. A client retrying a revoked key would fill the log, so revisit this when API's clients exist.

## Slice 2 — what was built

- `Cargo.toml` is a Cargo workspace. `rust-toolchain.toml` pins Rust 1.95.0.
- `apps/api` is the API service, built with axum 0.8.9, sqlx 0.9.0, and jsonwebtoken 11.1.0. Its binary is `api`, with two commands:
  - `api serve` answers `GET /health` and D9's `GET /v1/orgs/{organization_id}`.
  - `api migrate` applies `apps/api/migrations`, which holds no migration yet, per D11. The migrations are compiled into the binary, and `build.rs` makes Cargo rebuild when a migration file is added.
- `src/tokens.rs` verifies tokens against Auth's JWKS, per D12.
- `src/access.rs` holds `OrgCaller`, the axum extractor that checks the caller against the organization in the path, in D2's order. Every organization-scoped handler takes it, so D2's check runs before the handler body.
- `compose.yaml` adds `api-migrate`, after `auth-migrate`, and `api`, on `localhost:3002`.
- `apps/api/Dockerfile` builds a release binary on `rust:1.95.0-slim-trixie` and runs it on `debian:trixie-slim` as a non-root user. The image is 161 MB.

Choices made in the build, within the spec:

- An error response is JSON carrying a `code`: `unauthorized` (401, with `WWW-Authenticate: Bearer`), `not_found` (404, also for an unknown path), `method_not_allowed` (405), `auth_unavailable` (503), or `internal` (500). This is provisional until 00002 publishes the REST API schema.
- reqwest is built without its default TLS provider, so the image does not compile aws-lc. It uses ring, which sqlx already links. Both can reach Auth and PostgreSQL over TLS, so a hosted deployment changes only URLs.
- API does not send CORS headers. Web does not call API yet. The first Web page that does will need API to allow Web's origin, as Auth does.
- `api` handles SIGTERM, so `docker compose stop api` returns at once instead of waiting for Docker to kill it.

## Slice 3 — what was built

- `apps/worker` is the Worker service, built with `restate-sdk` 0.12.1, pinned exactly apart from the Restate server. Its binary is `worker`, with three commands:
  - `worker serve` serves a Restate endpoint with no services on port 9080, and stops on SIGTERM.
  - `worker register` registers `WORKER_URL` with Restate's admin API at `RESTATE_ADMIN_URL`, with `force`, per D14. It fails with Restate's error when Restate refuses.
  - `worker health` asks the endpoint's `/health` over HTTP/2. It is Worker's healthcheck.
- `compose.yaml` adds `worker` and the one-off `worker-register`, which waits for Restate and Worker to be healthy. Restate gains a healthcheck on its admin API.
- `apps/worker/Dockerfile` mirrors API's. The image is 157 MB.

Choices made in the build, within the spec:

- Worker's endpoint is not published outside Compose, and it does not check that requests come from Restate. Only Restate reaches it, over the Compose network. A hosted deployment that exposes Worker would configure the SDK's request identity check.
- Restate's ingress and admin ports stay unpublished. Nothing outside Compose calls them yet.
- `restate-sdk` is built without its default features. Worker needs only its HTTP server.
- The healthcheck is a Worker command, not a TCP connect. The endpoint speaks HTTP/2 only, and logs a warning for every connection that closes before sending a request.
- Both Rust Dockerfiles copy both crates, since the Cargo workspace needs every member present. Their cache mounts are `sharing=locked`. Compose builds the images in parallel, and two Cargo processes unpacking into one shared registry failed with `File exists`.

## Verified

Each item names the command that verified it, run on 2026-10-22 on an Apple Silicon Mac with Docker 29.8.2.

`pnpm --filter @pipewhere/auth test` (20 tests, all passing) covers these D2 checks:

- session-to-token exchange, with the user's `usr_` id in `sub` and no claims beyond `sub`, `iss`, `aud`, `iat`, and `exp`
- API-key-to-token exchange, with the key's `key_` id in `sub`
- a missing, unknown, disabled, or expired key refused
- a key created by a removed member still exchanging
- after a user creates an organization or accepts an invitation, a call that names no organization acting on neither
- an invitee whose email is not verified refused when accepting an invitation
- deleting an organization through Auth refused
- API's database role reading the granted columns, and refused on other columns, on other tables, and on an UPDATE and a DELETE. It holds no write privilege, since nothing grants one.
- Auth's database role refused on the `pipewhere` schema

The same run covers D6's prefixes on every Auth table that holds rows, the generator throwing for an unlisted model, and migrations running twice without change.

Removing the session hook makes three organization tests fail, so they test the hook rather than Better Auth's defaults.

`docker compose up -d --build`, from a `.env` made as the README says:

- All five long-running services start, and both one-off steps exit 0.
- Recreating the Restate container keeps its data, after `RESTATE_NODE_NAME` was set. Before that, it refused to start.
- In Chrome, against `http://localhost:3000`:
  - signing up
  - creating a personal and a company organization
  - getting a session token
  - opening the company organization
  - creating a key and exchanging it for a token

The README's steps, run word for word in a fresh clone of this branch, bring up the same stack. Auth answers `/health`, Web answers 200, and a sign-up succeeds. The run used `docker compose -p pipewhere-fresh` so it did not share volumes with the running stack.

`docker stats --no-stream`, with the stack idle after that walkthrough: Web 57 MiB, Auth 60 MiB, PostgreSQL 50 MiB, RustFS 89 MiB, Restate 278 MiB. That is about 530 MiB for five of the seven processes.

Review fixes then changed the PostgreSQL healthcheck, Auth's CORS and session reads, and Web's key list. Re-run on 2026-10-31 at `0c27734`, on the existing volumes:

- `docker compose up -d --build`: every service starts, PostgreSQL reports healthy over TCP, and both one-off steps exit 0.
- A CORS preflight from Web's origin allows only `content-type` and `x-api-key`.
- A signed-in `get-session` carries no `set-auth-jwt` header, and `/api/auth/token` still issues a token. Run in-process against the test database.
- In Chrome: signing up, creating an organization, which shows "No keys yet", and creating a key, which is listed after a reload.

The fresh-clone README walk and the memory figures above were not re-run on this head.

### Slice 2

Run on 2026-10-31 on the same machine, with `pnpm --filter @pipewhere/auth test:db` running.

`cargo test -p pipewhere-api`, 15 tests, all passing. `tests/access.rs` drives a real Auth, per D13, and covers these D2 checks:

- each of these reported as unauthorized:
  - no token, or one that is not a JWT
  - a token with no `sub`, or a `sub` with neither prefix
  - a token that is expired or has the wrong audience or issuer
  - a token whose signature is malformed, or is Auth's signature over other claims
- a disabled, expired, or deleted key reported as unauthorized while its token is still valid
- a user who is not a member, another organization's key, and an organization that does not exist, each reported as not found
- a member removed while their token is still valid, reported as not found on the next request
- a role change shown on the next request
- a key created by a removed member still working on the next request
- a session token and a key token each reading the organization, with the actor's id and the user's role
- tokens signed before and after a signing-key rotation both verifying
- API's migrations running twice
- an unknown path and a wrong method each answered with a JSON `code`

`tests/tokens.rs` covers D12's fetching, against a stand-in key endpoint:

- five distinct unknown key ids, one after another, causing one fetch
- sixteen concurrent requests with distinct unknown key ids causing one fetch
- an unknown key id fetching again once the cooldown ends
- three concurrent requests behind an Auth that never answers each reported as unavailable within 6 seconds in total
- an unreachable Auth reported as unavailable

Breaking the code shows the tests check it. Each of these makes its test fail:

- dropping the `enabled` check
- never refetching keys once some are loaded
- removing the cooldown
- letting concurrent misses fetch without waiting for each other
- a cooldown that never ends
- a timeout on Auth's response alone, which made those three requests take 15 seconds

Adding a migration file, and changing nothing else, makes `cargo build -p pipewhere-api` recompile API.

`cargo clippy -p pipewhere-api --all-targets -- -D warnings` and `cargo fmt --check` pass.

`docker compose up -d --build`, on the existing volumes:

- `api-migrate` exits 0 after `auth-migrate`, and `api` starts.
- `/health` answers, and a request with no token gets 401 with `WWW-Authenticate: Bearer`.
- A user signed up through Auth with curl. A session token, then a key token, each read the organization through API, which fetched the keys from `http://auth:3001` and checked the issuer `http://localhost:3001`. The key token got `not_found` for another organization.
- `docker compose stop api` returned in 0.15 seconds.
- `docker stats --no-stream`, right after a restart, with no requests since: API 2.6 MiB. This is not a footprint measurement under load; slice 4 measures the full stack.

### Slice 3

Run on 2026-11-01 on the same machine.

`cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`, and `pnpm fmt:check` pass.

The registration evidence in the spec's "Implementation constraints and evidence" was reproduced against a throwaway `restatedev/restate:1.7.13` container, with Worker running on the host.

`docker compose up -d --build`, on the existing volumes:

- `restate` and `worker` report healthy. `worker-register` exits 0, and Restate lists the deployment `http://worker:9080/` with `restate-sdk-rust/0.12.1` and no services.
- `docker compose up -d` again, with nothing changed, runs `worker-register` again. The deployment id stays the same.
- A `Probe` service with one handler, added to Worker for this check and then removed:
  - After `docker compose up -d --build`, Restate listed it, and a call through Restate's ingress returned `"pong"`.
  - After removing it and running the same command, the call returned 404.
- Restarting Restate keeps the deployment.
- `docker compose stop worker` returned in 0.29 seconds.
- With `WORKER_URL` overridden to a port nothing listens on, `worker-register` exits 1 with Restate's discovery error, and `docker compose up -d` still exits 0. `--wait` does not help: it treats every one-off nothing depends on as failed, even one that exits 0. So `docker compose up -d --wait` now exits 1 on a successful start too, and slice 4 should not use it until D15 applies.
- `docker compose up -d --build worker` recreates Worker without running `worker-register`. `docker compose up -d --build worker worker-register` runs it, and the README says so.
- `docker stats --no-stream`, idle: Worker 2.2 MiB. The checks above did not cause Restate's growth: a fresh `restatedev/restate:1.7.13` container, idle and with nothing registered, used 804 MiB 15 seconds after starting and 816 MiB at 60 seconds. The stack's Restate, about an hour after it started, used 1.1 GiB. Slice 1 recorded 278 MiB, and it is unknown whether that figure came from the same moment after start or from a since-changed Restate. Slice 4 measures this properly, and it bears on the intent's MacBook test.

Worker has no tests. It has no handler, and its two commands are a server with nothing bound and one call to Restate. A test of either needs a real Restate, so the checks above ran against one instead. The first handler, in 00003, needs a test harness with a Restate server.

## Not yet verified

- D2 checks that need a gated action. D10 moves them to the pipeline that adds the first one:
  - a member whose role does not permit an action, and a key whose permissions do not, reported as forbidden
  - a role change, or a key permission change, affecting what the caller may do on the next request
- D5's object-store checks. RustFS runs but nothing uses it yet.
- The intent's MacBook test. The numbers above come from a 14-core machine, and they predate API and Worker. Whether the stack gets in the way of development work is for the operator to judge on an entry-level MacBook Pro.
- A new operator following the README unaided. The builder followed it from a fresh clone, as described above, which is not the same test.
- A cold first build. The fresh-clone run reused Docker's layer cache from earlier builds, so the time a new machine takes to build the images is unmeasured.
- Web's key list for a member who may not list keys. It should show Auth's refusal, not "No keys yet". This was checked by reading the code, not in a browser.
