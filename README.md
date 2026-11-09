# pipewhere

## Run it locally

You need Docker with Compose v2.

1. Create your configuration. Every secret must be set; none has a default.

   ```sh
   cp .env.example .env
   for name in POSTGRES_PASSWORD AUTH_DB_PASSWORD API_DB_PASSWORD BETTER_AUTH_SECRET RUSTFS_ACCESS_KEY RUSTFS_SECRET_KEY; do
     sed -i.bak "s|^$name=\$|$name=$(openssl rand -hex 24)|" .env
   done
   sed -i.bak "s|^CREDENTIALS_KEY=\$|CREDENTIALS_KEY=$(openssl rand -hex 32)|" .env
   rm .env.bak
   ```

   `CREDENTIALS_KEY` encrypts the access Pipewhere stores for each channel. API and Worker both read it. Back it up with your database. If it is lost or changed, Pipewhere cannot read what it stored. Publications to every channel fail until each one is connected again.

   An `.env` made before Mastodon channels existed lacks `CREDENTIALS_KEY` and `API_URL`, and Compose refuses to start without them. Add each one only if it is missing, so a key already in use is never replaced:

   ```sh
   grep -q '^CREDENTIALS_KEY=.' .env || echo "CREDENTIALS_KEY=$(openssl rand -hex 32)" >> .env
   grep -q '^API_URL=.' .env || echo "API_URL=http://localhost:3002" >> .env
   ```

2. Start everything:

   ```sh
   docker compose up -d --build
   ```

3. Open <http://localhost:3000>, create an account, and create an organization.

   Compose publishes Web, Auth, and API on `127.0.0.1` only. If another server listens on port 3000, 3001, or 3002 on `::1`, such as a Node dev server, Compose does not report it, and `localhost` may reach that server instead. Stop it first.

4. On the organization's page, connect a Mastodon account: enter its instance's address, such as `mastodon.social`, and approve Pipewhere at the instance. Pipewhere registers itself with the instance, so there is no app to create first.

Compose starts PostgreSQL, RustFS, and Restate, then Auth, Worker, API, and Web. Before them, three one-off steps run in order and exit:

- `postgres-setup` creates the `auth` and `pipewhere` schemas and the two database roles.
- `auth-migrate` applies Auth's migrations and grants API's role the columns it may read.
- `api-migrate` applies API's migrations.

Once Restate and Worker are healthy, a fourth step, `worker-register`, registers Worker's handlers with Restate, so Restate picks up changed handlers. API starts only after it succeeds, so a failed registration stops `docker compose up`, and `docker compose logs worker-register` shows why.

To rebuild Worker alone, name the registration step too. Compose starts a named service's dependencies, not the services that depend on it, so `docker compose up -d --build worker` alone leaves Restate with Worker's old handlers:

```sh
docker compose up -d --build worker worker-register
```

Web listens on port 3000, Auth on port 3001, and API on port 3002, all on `127.0.0.1`.

Worker runs each publication when its time comes:

- Every 5 seconds it looks in PostgreSQL for `scheduled` publications whose planned time has passed, and sends up to 100 of them to Restate, oldest first. A failed tick is logged, and the next one tries again.
- It posts to a connected Mastodon account. A Threads channel cannot publish yet, so its publication becomes `failed`, with the reason `Pipewhere cannot publish to Threads yet.`
- If the instance no longer accepts Pipewhere's access, the publication fails and the channel shows its access as revoked. Connect the account again, then retry.

API serves its OpenAPI schema at `http://localhost:3002/openapi.json`.

To call API, open an organization in Web and create a key. Web shows the key once. The organization's id is the last part of the page's URL. Exchange the key at Auth for an access token, then name the organization in API's path:

```sh
KEY=... # the key Web showed
ORGANIZATION_ID=... # from the URL, starting org_
TOKEN=$(curl -s -X POST -H "x-api-key: $KEY" http://localhost:3001/api-key/token | sed 's/.*"token":"\([^"]*\)".*/\1/')
curl -H "authorization: Bearer $TOKEN" http://localhost:3002/v1/orgs/$ORGANIZATION_ID
```

A token expires after 15 minutes. Exchange the key again for a new one.

Stop the stack with `docker compose down`. Your data stays in Docker volumes; `docker compose down --volumes` deletes it.

## Change Worker

An invocation in flight resumes on the new code when Worker's registration is forced, which every start of the stack does. This is safe only while a run's journal keeps its shape, so:

- You may change what a step does. Restate replays the steps it has recorded, and the next step runs on the new code.
- You may not add another kind of journal entry, such as a sleep or a call, rename the `step` entry, or change its return type. Doing so needs Restate's versioned deployments, or a drain that waits for runs in flight to end, per [00003 D6](docs/pipelines/00003-scheduling-recovery/spec.md).

## Develop

Install dependencies with `pnpm install`. Node and pnpm versions are pinned in `.prototools`, and Rust's in `rust-toolchain.toml`.

Auth's, API's, and Worker's tests run against a disposable PostgreSQL server and need `psql` on your path. API's and Worker's tests also start Auth with Node, so run `pnpm install` first. Worker's tests start Restate's image with Docker, one container per test:

```sh
pnpm --filter @pipewhere/auth test:db   # starts the server on port 5433
pnpm --filter @pipewhere/auth test
pnpm --filter @pipewhere/auth typecheck
pnpm --filter @pipewhere/web typecheck
cargo test -p pipewhere-api
cargo test -p pipewhere-worker
cargo clippy --workspace --all-targets -- -D warnings
```

Rerun API's tests whenever Auth's migrations or Better Auth's version change. API reads Better Auth's tables and verifies its tokens, so an upgrade can break API without breaking Auth.

Stop the test server with `docker stop pipewhere-test-postgres`.

## License

Source available under the [Elastic License 2.0](LICENSE).
