# Plan 00001 — Self-hostable infrastructure foundation

The build runs in slices. Each slice leaves the stack running from one command.

## Slices

1. **Infrastructure and Auth, with a first Web UI.** Built.
2. **API.** Not started. Local token verification, the per-request organization check from D2, the `pipewhere` schema's migrations with D6's id checks, and the cross-generator prefix test.
3. **Worker.** Not started. Restate handlers, with the server and the Rust SDK pinned separately. Reproduce deployment re-registration in the pinned Restate version before choosing how Worker registers.
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

## Not yet verified

- D2 checks that need API: unauthorized, not-found, and forbidden responses; membership, role, and key changes taking effect on the next request; and verification across a signing-key rotation.
- D5's object-store checks. RustFS runs but nothing uses it yet.
- The intent's MacBook test. The numbers above come from a 14-core machine, and API and Worker are missing. Whether the stack gets in the way of development work is for the operator to judge on an entry-level MacBook Pro.
- A new operator following the README unaided. The builder followed it from a fresh clone, as described above, which is not the same test.
- A cold first build. The fresh-clone run reused Docker's layer cache from earlier builds, so the time a new machine takes to build the images is unmeasured.
- Web's key list for a member who may not list keys. It should show Auth's refusal, not "No keys yet". This was checked by reading the code, not in a browser.
- A key whose permissions do not allow an action. Auth has no endpoint that sets a key's permissions yet. API will need one before it can check them.
