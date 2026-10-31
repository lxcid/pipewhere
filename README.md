# pipewhere

## Run it locally

You need Docker with Compose v2.

1. Create your configuration. Every secret must be set; none has a default.

   ```sh
   cp .env.example .env
   for name in POSTGRES_PASSWORD AUTH_DB_PASSWORD API_DB_PASSWORD BETTER_AUTH_SECRET RUSTFS_ACCESS_KEY RUSTFS_SECRET_KEY; do
     sed -i.bak "s|^$name=\$|$name=$(openssl rand -hex 24)|" .env
   done
   rm .env.bak
   ```

2. Start everything:

   ```sh
   docker compose up -d --build
   ```

3. Open <http://localhost:3000>, create an account, and create an organization.

Compose starts PostgreSQL, RustFS, and Restate, then Auth, API, and Web. Before Auth and API start, three one-off steps run in order and exit:

- `postgres-setup` creates the `auth` and `pipewhere` schemas and the two database roles.
- `auth-migrate` applies Auth's migrations and grants API's role the columns it may read.
- `api-migrate` applies API's migrations.

Web listens on port 3000, Auth on port 3001, and API on port 3002, all on localhost only. The Worker service is not built yet.

To call API, open an organization in Web and create a key. Web shows the key once. The organization's id is the last part of the page's URL. Exchange the key at Auth for an access token, then name the organization in API's path:

```sh
KEY=... # the key Web showed
ORGANIZATION_ID=... # from the URL, starting org_
TOKEN=$(curl -s -X POST -H "x-api-key: $KEY" http://localhost:3001/api-key/token | sed 's/.*"token":"\([^"]*\)".*/\1/')
curl -H "authorization: Bearer $TOKEN" http://localhost:3002/v1/orgs/$ORGANIZATION_ID
```

A token expires after 15 minutes. Exchange the key again for a new one.

Stop the stack with `docker compose down`. Your data stays in Docker volumes; `docker compose down --volumes` deletes it.

## Develop

Install dependencies with `pnpm install`. Node and pnpm versions are pinned in `.prototools`, and Rust's in `rust-toolchain.toml`.

Auth's and API's tests run against a disposable PostgreSQL server and need `psql` on your path. API's tests also start Auth with Node, so run `pnpm install` first:

```sh
pnpm --filter @pipewhere/auth test:db   # starts the server on port 5433
pnpm --filter @pipewhere/auth test
pnpm --filter @pipewhere/auth typecheck
pnpm --filter @pipewhere/web typecheck
cargo test -p pipewhere-api
cargo clippy -p pipewhere-api --all-targets -- -D warnings
```

Rerun API's tests whenever Auth's migrations or Better Auth's version change. API reads Better Auth's tables and verifies its tokens, so an upgrade can break API without breaking Auth.

Stop the test server with `docker stop pipewhere-test-postgres`.

## License

Source available under the [Elastic License 2.0](LICENSE).
