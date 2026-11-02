# Plan 00002 — Publication contract

The build runs as one small PR per issue. Each issue carries its own scope and dependencies. This plan records what each one built as it lands.

## Steps

1. **#13 API's id generator.** Built in PR #19.
2. **#14 Channels table and read endpoints.** Not started. Needs #13.
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
