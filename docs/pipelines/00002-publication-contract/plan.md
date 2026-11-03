# Plan 00002 — Publication contract

The build runs as one small PR per issue. Each issue carries its own scope and dependencies. This plan records what each one built as it lands.

## Steps

1. **#13 API's id generator.** Built in PR #19.
2. **#14 Channels table and read endpoints.** Built in PR #20.
3. **#15 Content create, read, and edit.** Not started. Needs #13.
4. **#16 Create and read publications.** Not started. Needs #14 and #15.
5. **#17 Edit, cancel, retry, and resolve publications.** Not started. Needs #16.
6. **#18 Publication attempts table and read endpoint.** Not started. Needs #16.

## #13 — what was built

- `apps/api/src/ids.rs` holds `PREFIXES`, which maps each `pipewhere` table to its prefix. `new_id(table)` returns the prefix, an underscore, and a UUIDv7 as 26 lowercase Crockford base32 characters, per 00001 D6. It panics for a table with no listed prefix, as Auth's generator throws.
- The migration `20261102000000_is_typeid.sql` adds `pipewhere.is_typeid(id, prefix)`. Each table's migration checks its id with `CHECK (pipewhere.is_typeid(id, '<prefix>'))`.
- `apps/api/tests/ids.rs` reads Auth's `authPrefixes` through `node` and fails when Auth and API share a prefix, or when a prefix is not 3 to 5 lowercase letters. It also checks `is_typeid` against generated ids and malformed ones.
- Both test harnesses hold one advisory lock in the `postgres` database while `setup.sql` runs. `setup.sql` alters roles the whole server shares, so concurrent test runs failed with `tuple concurrently updated`.

Choices made in the build, within the spec:

- Prefixes: `chn` for `channel`, `cnt` for `content`, `pub` for `publication`, and `pubat` for `publication_attempt`. They are listed before their tables exist so the collision test has real entries to compare. The operator chose `pubat` so that an attempt's id starts like its publication's `pub`. `att` was rejected because it reads as "attachment", and a later media table could not reuse it.
- The base32 encoding is written by hand on the `uuid` crate instead of adding a TypeID crate. A test pins it to three ids encoded by typeid-js 1.2.0, the library Auth uses.
- A migration's version is the time its file was created, to the second, as `sqlx migrate add` writes it. That lets #14 and #15 be built in parallel without clashing. The first migration's version, `20261102000000`, carries only the date. Do not copy that pattern. Two migrations on one version fail when sqlx records the second.
- Tolerated: nothing checks that a table actually carries its id check. A wrong prefix in a check fails at the table's first insert test. A missing check is caught only in review.

Verified after the `pubat` rename. The lock probes ran at `aff58e8`, and neither harness has changed since.

- `cargo test -p pipewhere-api`: 5 unit tests and 17 integration tests pass.
- `pnpm --filter @pipewhere/auth test`: 20 tests pass.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check`: clean.
- The collision test fails when a shared prefix is added by hand. `("clash", "org")` gives `auth.organization and pipewhere.clash share the prefix "org"`.
- Four concurrent `access` test binaries plus Auth's suite all pass. Before the lock, four concurrent `access` binaries all failed.
- With `TEST_DATABASE_URL` naming another database, Auth's tests wait for a lock held in `postgres`: about 8 seconds with the lock held, about 0 without.

Asserted, not verified:

- The API Docker image embeds the new migration. The image was not built. Its Dockerfile copies all of `apps/api`.

## #14 — what was built

- The migration `20261102171234_channel.sql` adds `pipewhere.channel` with `id`, `organization_id`, and `provider`:
  - The id is checked with `pipewhere.is_typeid(id, 'chn')`.
  - `organization_id` references `auth.organization` without a cascade, per 00001 D2.
  - `provider` is checked against D8's list.
- `apps/api/src/channels.rs` serves `GET /v1/orgs/{organization_id}/channels` and `GET /v1/orgs/{organization_id}/channels/{channel_id}`. Both take `OrgCaller`. Both queries filter on the path's organization, so another organization's channel is not found, per D6.
- Routes and schema paths are registered in one place. `api()` in `apps/api/src/lib.rs` builds a utoipa-axum `OpenApiRouter`. Each `routes!` call serves a handler and adds its path to the schema, and `/openapi.json` serves the schema that router produced. `ApiDoc` now holds only the schema's info and the bearer scheme.
- `apps/api/tests/channels.rs` inserts channel rows directly, since connecting a channel belongs to 00004.

Choices made in the build, within the spec:

- The list answers `{ "channels": [...] }`, not a bare array, so a later cursor can be added without breaking clients. Channels are ordered by id, which is creation order to the millisecond, per 00001 D6.
- The channel's path parameter is `channel_id`, to match `organization_id`. The issue wrote `{id}`.
- API moves from utoipa 5.5.0 to 6.0.0, because utoipa-axum 0.3.0 requires it. The upgrade needed no code change, and it drops `paste` from `Cargo.lock`.
- No index on `organization_id`. An organization has a handful of channels. Organization deletion is off, so the foreign key's delete check does not run in normal use. Add one when a measured list query needs it.
- Tolerated: nothing stops a later route being added with `OpenApiRouter::route`, which serves it without documenting it. `the_schema_lists_every_endpoint` compares the schema with a written list, not with the routes served.
- Tolerated: a channel holds only its provider, so two channels on one provider look alike in the API. 00004 adds the account each one links.

Verified:

- `cargo test -p pipewhere-api`: 5 unit tests and 22 integration tests pass, 5 of them in `tests/channels.rs`.
- `pnpm --filter @pipewhere/auth test`: 20 tests pass.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check`: clean.
- With the single-channel query's organization filter removed, `another_organizations_channel_is_not_found` fails. Another organization's channel then answers 200.
- The table refuses an id with another table's prefix (`channel_id_check`), an unlisted provider (`channel_provider_check`), and an organization that does not exist (`channel_organization_id_fkey`). Deleting an organization that has a channel fails on `channel_organization_id_fkey`.
- `api serve`, run by hand against the test server, served `/openapi.json` with all four paths. It answered an unauthenticated channel list with 401.
- `/openapi.json` served by a utoipa 5.5.0 build and by a 6.0.0 build of the same code compared equal as JSON. Both builds were run by hand against the test server.
- A throwaway test printed the `Provider` schema as a string enum of `mastodon` and `threads`.

Asserted, not verified:

- The API Docker image embeds the new migration. The image was not built, as for #13.
