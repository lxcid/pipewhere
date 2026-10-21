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

Compose starts PostgreSQL, RustFS, and Restate, then Auth and Web. Before Auth starts, two one-off steps run and exit:

- `postgres-setup` creates the `auth` and `pipewhere` schemas and the two database roles.
- `auth-migrate` applies Auth's migrations and grants API's role the columns it may read.

Web listens on port 3000 and Auth on port 3001, both on localhost only. The API and Worker services are not built yet.

Stop the stack with `docker compose down`. Your data stays in Docker volumes; `docker compose down --volumes` deletes it.

## Develop

Install dependencies with `pnpm install`. Tool versions are pinned in `.prototools`.

Auth's tests run against a disposable PostgreSQL server and need `psql` on your path:

```sh
pnpm --filter @pipewhere/auth test:db   # starts the server on port 5433
pnpm --filter @pipewhere/auth test
pnpm --filter @pipewhere/auth typecheck
pnpm --filter @pipewhere/web typecheck
```

Stop the test server with `docker stop pipewhere-test-postgres`.

## License

Source available under the [Elastic License 2.0](LICENSE).
