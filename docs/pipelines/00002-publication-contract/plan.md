# Plan 00002 — Publication contract

The build runs as one small PR per issue. Each issue carries its own scope and dependencies. This plan records what each one built as it lands.

## Steps

1. **#13 API's id generator.** Built in PR #19.
2. **#14 Channels table and read endpoints.** Built in PR #20.
3. **#15 Content create, read, and edit.** Built in PR #21.
4. **#16 Create and read publications.** Built in PR #22.
5. **#17 Edit, cancel, retry, and resolve publications.** Built in PR #23.
6. **#18 Publication attempts table and read endpoint.** Built in PR #24.

## Outcome check

Each bullet of the intent's proposed outcome, against what #13–#18 built. The tests named here passed in `cargo test -p pipewhere-api` at #18's head. Worker behavior is not built, so the parts only the worker can make true rest on binding decisions, not on tests.

- **One name and one meaning for each entity and state, including an attempt that explains failures and unknown outcomes.** D1, D3, and D19. The schema publishes `PublicationState` and `AttemptResult` as fixed sets, checked by `the_schema_publishes_the_error_codes_and_states_as_fixed_sets`. `the_table_refuses_an_attempt_it_cannot_explain` shows the table refusing a `failed` or `outcome_unknown` attempt without a reason. That the worker writes one for every failure is D19's binding on 00003 and 00004.
- **Each channel's publication shows exactly what goes out, and changing its content cannot change it.** D2. `a_member_creates_lists_and_reads_publications` edits content and reads the publication's copy unchanged. `an_edit_changes_only_this_channels_copy` edits the publication and reads the content unchanged. `resolving_records_what_a_person_found` refuses every action on a `published` publication.
- **The transitions distinguish work that is safe to repeat from a provider commit whose outcome may be unknown.** D3 and D16. `each_action_is_allowed_only_in_the_states_d3_lists` checks every person's action against every state: nothing edits or cancels `publishing`, and nothing retries `outcome_unknown`. The worker's transitions are bound by D3 and D4 on 00003 and 00004.
- **Repeating a client request returns the first result, and no automatic second commit follows an ambiguous result.** D5, D13, and D17. `a_repeated_request_returns_the_first_result`, `concurrent_use_of_one_key_creates_one_publication`, `a_repeated_retry_returns_the_publication_without_retrying_again`, and `concurrent_requests_change_a_publication_once`. The API answers 409 to a retry of `outcome_unknown`. No automatic commit is D4's binding on 00004.
- **Every publication belongs to one organization, and a single operator needs no team setup.** D6. Each resource has an `another_organization…_not_found` test, through #18's `another_organizations_attempts_are_not_found`. Every channel, content, publication, and attempt test acts as the sole member of an organization its owner just created.

Not part of the outcome, and open: the API Docker image embedding the migrations was never built, and a NUL in the path's organization id answers 500 on every endpoint.

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

## #15 — what was built

- The migration `20261103043042_content.sql` adds `pipewhere.content` with `id`, `organization_id`, and `body`:
  - The id is checked with `pipewhere.is_typeid(id, 'cnt')`.
  - `organization_id` references `auth.organization` without a cascade, per 00001 D2.
  - `body` is plain text with no table check, per D10.
- `apps/api/src/contents.rs` serves three operations. All take `OrgCaller`, and every query filters on the path's organization, per D6:
  - `POST /v1/orgs/{organization_id}/contents` creates content and answers 201 with it.
  - `GET /v1/orgs/{organization_id}/contents/{content_id}` reads one.
  - `PATCH /v1/orgs/{organization_id}/contents/{content_id}` replaces its body and answers 200 with it.
- Request and response share one shape: `{ "body": "..." }` in, `{ "id", "organization_id", "body" }` out.
- `JsonBody` in `apps/api/src/error.rs` wraps axum's `Json`. A body API cannot read answers `invalid_body` with axum's status, per D9. Blank text, or text holding U+0000, answers 422 `invalid_body`, per D10.
- `support::Instance` gains `post` and `patch`, which send a JSON body.

Choices made in the build, within the spec:

- D5 does not reach content. Its retry is a publication's `failed` → `scheduled`, and its violation clause names publications. A lost-response retry of content create makes a second private row, which publishes nothing. Creating content from a publication request is #16's, and D5 covers that request.
- `PATCH` requires `body`, so it replaces the one editable field. When content gains a second field, `body` becomes optional, which existing clients do not notice.
- Deferred: listing content. A list grows without bound, so it needs a cursor, which no client needs yet. Add it with the first client that browses content.
- Tolerated: content has no delete. A duplicate from a retried create stays as a private row.
- Tolerated: `PATCH` reads the body before looking up the content. Any body D9 or D10 refuses, sent to missing content, answers `invalid_body`, not 404. The caller has already been checked against the organization, so this reveals nothing.
- No index beyond the primary key. Reads and edits go by id.

Verified:

- `cargo test -p pipewhere-api`: 5 unit tests and 28 integration tests pass, 6 of them in `tests/contents.rs`.
- `pnpm --filter @pipewhere/auth test`: 20 tests pass.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check`: clean.
- `a_body_api_cannot_read_is_refused` sends each bad body to `POST` and `PATCH`: not JSON (400), no content type (415), no `body` field and a non-text `body` (422), empty and whitespace-only text and text holding U+0000 (422), and over 2 MB (413). Each answers `invalid_body`, and the stored row is unchanged. An outsider's malformed body answers 404.
- Each of these changes, made by hand and reverted, fails a test:
  - The read query's organization filter removed: `another_organizations_content_is_not_found`.
  - The edit query's organization filter removed: the same test.
  - `trim()` dropped from the blank check: `a_body_api_cannot_read_is_refused`.
  - The NUL check absent: the same test, with 500 `internal` for text holding U+0000.
  - Every rejection mapped to 400: the same test.
- The table refuses an id with another table's prefix (`content_id_check`) and an organization that does not exist (`content_organization_id_fkey`). Deleting an organization that has content fails on `content_organization_id_fkey`.
- `api serve`, run by hand against the test server, served `/openapi.json` with `post` on `/contents`, and `get` and `patch` on `/contents/{content_id}`. It answered an unauthenticated malformed `POST` with 401.

Asserted, not verified:

- The API Docker image embeds the new migration. The image was not built, as for #13 and #14.

## #16 — what was built

- The migration `20261103052937_publication.sql` adds two tables:
  - `pipewhere.publication` holds `id`, `organization_id`, `channel_id`, `content_id`, `body`, `state`, and `planned_at`. The id is checked with `pipewhere.is_typeid(id, 'pub')`. `state` is checked against D3's seven states. `organization_id` references `auth.organization` without a cascade, per 00001 D2.
  - `pipewhere.idempotency_key` holds `organization_id`, `key`, `request_hash`, and `publication_id`, keyed by `(organization_id, key)`, per D13. It has no id of its own, per 00001 D6. Its reference to the publication is checked at commit.
- `apps/api/src/publications.rs` serves three operations. All take `OrgCaller`, and every query filters on the path's organization, per D6:
  - `POST /v1/orgs/{organization_id}/publications` requires `Idempotency-Key`, and answers 201 with the publication, per D11–D13.
  - `GET /v1/orgs/{organization_id}/publications` lists them newest first, 100 to a page, with `next_cursor`, per D14.
  - `GET /v1/orgs/{organization_id}/publications/{publication_id}` reads one.
- A publication id, a body's `channel_id` or `content_id`, or a cursor holding U+0000 does not reach PostgreSQL, whose text cannot store it. The reference answers its `*_not_found` code, the publication id 404, and the cursor an empty page. The path's organization id still reaches PostgreSQL through `OrgCaller` and answers 500, as on every endpoint; PR #22's review follow-ups record it.
- `ErrorBody.code` is now `ErrorCode`, so the schema publishes every code as a string enum. #16 adds four codes: `invalid_idempotency_key` (400), `idempotency_key_reused` (422), `channel_not_found` (422), and `content_not_found` (422).
- `PublicationState` publishes D3's seven states. Create sets only `scheduled`.
- D10's text check moved to `contents::checked_body`, which content and publication create share.
- `support::Instance` gains `request`, which returns a request builder so a test can add headers.

Choices made in the build, within the spec:

- API moves timestamps through `chrono` 0.4.45, with sqlx's and utoipa's `chrono` features. The request hash uses `sha2` 0.11.1, the version sqlx already pulls in.
- Each new error code adds a value to the published `ErrorCode` enum, with D8's cost: a client that validates the enum strictly fails to read an error with a code added after it was generated.
- The list reads `?cursor=` through a map, which cannot fail to deserialize. A struct would answer a repeated `cursor` with axum's plain-text rejection.
- Tolerated: the database does not check that a publication's channel and content belong to its organization. API checks both in the transaction that inserts it, and API is the only writer. A composite foreign key would need a second unique index on `channel` and on `content`.
- Tolerated: a cursor that is not a publication id is compared as text. It answers a page, possibly empty, instead of a refusal.
- No index beyond the primary keys. The list walks the publication primary key backwards and filters by organization. Add an index on `(organization_id, id)` when a measured list query needs it, as for #14.

Verified:

- `cargo test -p pipewhere-api`: 6 unit tests and 37 integration tests pass, 9 of them in `tests/publications.rs`.
- `pnpm --filter @pipewhere/auth test`: 20 tests pass.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check`: clean.
- `the_schema_publishes_the_error_codes_and_states_as_fixed_sets` checks the `ErrorCode` and `PublicationState` enums and the `date-time` format of `planned_at`.
- A throwaway test fetched `/openapi.json` from the router `api serve` runs. It listed both new paths, the required `Idempotency-Key` header, the optional `cursor` query parameter, and `ErrorBody.code` as a reference to `ErrorCode`.
- `concurrent_use_of_one_key_creates_one_publication` sends 20 requests with one key at once. All answer 201 with one id, and one publication and one content row exist.
- `a_body_api_cannot_read_is_refused` also refuses with 422 `invalid_body`: an unknown field (`scheduled_at`), `+10000-01-01T00:00:00Z`, `9999-12-31T23:59:59-23:59` (year 10000 in UTC), `9999-12-31T23:59:60Z` (a leap second that carries into 10000 when stored), and `0000-01-01T00:00:00+00:01` (year −1 in UTC). It refuses forms chrono's own deserializer accepts but RFC 3339 does not: `2026-1-1T9:0:0Z`, `2026-12-01T09:00:00+0800`, and `2026-12-01T09:00:00UTC`.
- `a_missing_or_short_key_is_refused` also refuses a 30-byte key that is not visible ASCII with 400 `invalid_idempotency_key`.
- `a_request_waits_for_another_transaction_holding_its_key` holds the key in an open transaction. The request is still waiting after 500 ms. When the transaction commits a different request, it answers 422 `idempotency_key_reused`. When the transaction rolls back, it creates.
- Each of these changes, made by hand and reverted, fails a test:
  - The organization filter removed from the single read, the list, the channel check, or the content lookup: `another_organizations_resources_are_not_found`.
  - The organization filter removed from the stored key's lookup: `a_repeated_request_returns_the_first_result`.
  - The hash comparison skipped: the same test, which then returns the first publication for a different request.
  - `deny_unknown_fields` removed from `PublicationInput`: `a_body_api_cannot_read_is_refused`, where a body with `scheduled_at` answers 201.
  - The `planned_at` year check removed: the same test, where `9999-12-31T23:59:59-23:59` answers 201. The strict parser refuses five-digit years before the check runs, so the check guards only times an offset or a leap second carries out of range.
  - Only the check's lower bound removed: the same test, where `0000-01-01T00:00:00+00:01` answers 201.
  - The strict RFC 3339 parser removed from `planned_at`: the same test, where `2026-1-1T9:0:0Z` answers 201.
  - The U+0000 check on ids removed: `another_organizations_resources_are_not_found`, with 500 `internal`.
  - The key claimed by checking for it first and then inserting without `ON CONFLICT`: `a_request_waits_for_another_transaction_holding_its_key` fails on every run, with 500 `internal`. `concurrent_use_of_one_key_creates_one_publication` failed on 3 of 5 runs. Racing real requests depends on timing, which is why the held-transaction test exists.
- The tables refuse an id with another table's prefix (`publication_id_check`), a state outside D3 (`publication_state_check`), an organization that does not exist (`publication_organization_id_fkey`), and a key whose publication is never inserted (`idempotency_key_publication_id_fkey`, at commit). Both foreign keys to `auth.organization` have no delete action, read from `pg_constraint`.

Not isolated by a test:

- The organization filter on the stored publication's read. A stored key's publication id comes from a lookup that already filters on the organization, so removing the second filter changes nothing a test can see. It stays because every query filters on the organization.

Asserted, not verified:

- The API Docker image embeds the new migration. The image was not built, as for #13–#15.
- `api serve` was not run by hand. The schema was read from the same router through the test harness.

## #17 — what was built

- The migration `20261103083748_publication_remote.sql` adds `remote_id` and `remote_url` to `pipewhere.publication`. `publication_remote_id_check` requires `remote_id` exactly when the state is `published`, per D18.
- `apps/api/src/publications.rs` serves four operations, per D15. All take `OrgCaller`, and every query filters on the path's organization, per D6:
  - `PATCH /v1/orgs/{organization_id}/publications/{publication_id}` edits `body`, `planned_at`, or both, in `scheduled` or `failed`.
  - `POST .../{publication_id}/cancel` cancels from `scheduled` or `preparing`.
  - `POST .../{publication_id}/retry` requires `Idempotency-Key`, and moves `failed` to `scheduled`, per D17.
  - `POST .../{publication_id}/resolve` moves `outcome_unknown` to `published`, with a remote id and an optional address, or to `failed`, per D18.
- Each action is one conditional `UPDATE` on the states D3 allows. When it changes nothing, `refusal` looks the publication up: missing answers 404, and present answers 409 `state_conflict`, a new `ErrorCode`, per D16.
- Every publication response gains `remote_id` and `remote_url`, null until `published`.
- Create's year check on `planned_at` moved to `checked_planned_at`, which create, edit, and retry share. The publication column list is the `columns!` macro, since every query returning a publication now names nine columns.
- The schema publishes the resolution's `outcome` as the string enum `ResolutionOutcome`, `published` or `failed`.

Choices made in the build, within the spec:

- The tests are in `tests/publication_transitions.rs`, apart from #16's `tests/publications.rs`. They insert publications in each state directly, because only the worker moves a publication into `preparing`, `publishing`, or `outcome_unknown`.
- Tolerated: nothing checks that `remote_url` is set only with `remote_id`. Resolve is the only writer, and it sets both in one update.
- No index added. Every action goes by the publication's primary key.

Verified:

- `cargo test -p pipewhere-api`: 6 unit tests and 46 integration tests pass, 9 of them in `tests/publication_transitions.rs`.
- `pnpm --filter @pipewhere/auth test`: 20 tests pass.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check`: clean.
- `tests/publications.rs` and `tests/publication_transitions.rs` passed 5 runs in a row.
- `each_action_is_allowed_only_in_the_states_d3_lists` runs all five actions (edit, cancel, retry, and resolve to each outcome) against all seven states. Each allowed pair reaches D16's target state. Every other pair answers 409 `state_conflict` and leaves the publication unchanged.
- `resolving_records_what_a_person_found` refuses every action on a `published` publication, per D2. It refuses `javascript:alert(1)` and an address that is not a URL, and it stores `HTTPS://Mastodon.Example/@me/109` as `https://mastodon.example/@me/109`.
- `a_repeated_retry_returns_the_publication_without_retrying_again` retries once. It sets the publication back to `failed`, then repeats the request: the answer is `failed`, and the publication stays `failed`. The same key with another time, with no time, on another publication, or after a create, answers 422 `idempotency_key_reused`. A retry refused with 409 or 404 leaves its key unused.
- `concurrent_requests_change_a_publication_once` sends 20 retries with one key at once, and all answer 200 with the same publication. It sends 20 retries with different keys, and 20 cancels: each time, one answers 200 and 19 answer 409.
- `an_action_waits_for_another_transaction_changing_the_publication` holds an open transaction that moves the publication on, as the worker's claim would. An edit of a `scheduled` publication, and a cancel of a `preparing` one, still wait after 500 ms. When the transaction commits, they answer 409. When it rolls back, they succeed.
- `a_retry_waits_for_another_transaction_holding_its_key` does the same for a held key. On commit the retry answers 422 `idempotency_key_reused`. On rollback, it retries.
- A throwaway test fetched `/openapi.json` from the router the harness serves. It listed `patch` on the publication, `post` on `/cancel`, `/retry`, and `/resolve`, the required `Idempotency-Key` header on retry, and 409 on all four. `remote_id` and `remote_url` were required and nullable.
- Each of these changes, made by a script and reverted, fails a test:
  - The organization filter removed from the edit, cancel, retry, or resolve update, or from `refusal`'s lookup: `another_organizations_publication_is_not_found`.
  - The organization filter removed from the retry's stored key lookup: `a_repeated_retry_returns_the_publication_without_retrying_again`.
  - Each state guard removed: `each_action_is_allowed_only_in_the_states_d3_lists`, among others. Edit widened to `preparing`, or cancel widened to `publishing`: the same test, and `an_action_waits_for_another_transaction_changing_the_publication`.
  - The retry's hash comparison skipped: `a_repeated_retry_returns_the_publication_without_retrying_again` and `a_retry_waits_for_another_transaction_holding_its_key`.
  - The publication id, or the planned time, dropped from the retry's hash: `a_repeated_retry_returns_the_publication_without_retrying_again`.
  - The key claimed by checking for it first and then inserting without `ON CONFLICT`: `a_retry_waits_for_another_transaction_holding_its_key` and `concurrent_requests_change_a_publication_once`.
  - The key never claimed: five tests.
  - The U+0000 check on the publication id removed from any of the four: `another_organizations_publication_is_not_found`, with 500 `internal`.
  - D10's text check removed from the edit, D12's year check removed from the edit or the retry, or `deny_unknown_fields` removed from `PublicationEdit`: the edit's or retry's test.
  - The URL scheme check removed, or a blank or U+0000 remote id allowed: `resolving_records_what_a_person_found`.
- `a_published_publication_names_its_post` shows the table refusing `published` without a remote id, and a remote id on `failed`.

Not isolated by a test:

- The organization filter on the retry's stored publication read. As in create, the stored key's lookup already filters on the organization.
- `retry_publication` as the hash's first element. Create's hash has five elements, or two when it carries a tolerance, and retry's three, so the two never match without the names. The name keeps them apart if the shapes ever line up.

Asserted, not verified:

- The API Docker image embeds the new migration. The image was not built, as for #13–#16.
- `api serve` was not run by hand. The schema was read from the same router through the test harness.

## #18 — what was built

- The migration `20261103093910_publication_attempt.sql` adds `pipewhere.publication_attempt` with `id`, `publication_id`, `result`, `reason`, `started_at`, and `ended_at`, per D19:
  - The id is checked with `pipewhere.is_typeid(id, 'pubat')`.
  - `publication_id` references `pipewhere.publication` with no delete action. The attempt holds no `organization_id`.
  - `result` is checked against `published`, `failed`, and `outcome_unknown`.
  - `publication_attempt_ended_check` requires `result` and `ended_at` to be null together.
  - `publication_attempt_reason_check` requires a reason when the result is `failed` or `outcome_unknown`.
- `apps/api/src/publications.rs` serves `GET /v1/orgs/{organization_id}/publications/{publication_id}/attempts`, per D20. It takes `OrgCaller`. The attempts query joins the publication and filters on the path's organization. When it finds no attempts, a second query looks the publication up in the organization: missing answers 404, and present answers an empty list.
- The schema publishes `PublicationAttempt`, with `result`, `reason`, and `ended_at` required and nullable, and `AttemptResult` as a string enum.

Choices made in the build, within the spec:

- The tests are in `tests/publication_attempts.rs`. They insert attempts directly, since no worker writes them yet.
- No index beyond the primary key. The list scans attempts for the publication's id. At 10 publications a day and a few attempts each, the table holds tens of thousands of rows after years. Add an index on `publication_id` when a measured list query needs it, as for #14 and #16.
- Tolerated: nothing stops two running attempts for one publication. The worker is the only writer, and 00003 decides when an attempt starts.

Verified:

- `cargo test -p pipewhere-api`: 6 unit tests and 49 integration tests pass, 3 of them in `tests/publication_attempts.rs`.
- `pnpm --filter @pipewhere/auth test`: 20 tests pass.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check`: clean.
- `the_list_shows_each_attempt_newest_first` lists an empty publication as `{ "attempts": [] }`. It then lists a `failed`, an `outcome_unknown`, and a running attempt newest first, with each field as stored, and leaves out another publication's attempt.
- `another_organizations_attempts_are_not_found` answers 404 `not_found` for another organization's publication with attempts and without, a publication that does not exist, an id that is not a publication id, an id holding U+0000, and a user who is not a member.
- `the_table_refuses_an_attempt_it_cannot_explain` shows the table refusing another table's prefix, a result outside the list (`cancelled`), a result without an end and an end without a result, `failed` and `outcome_unknown` without a reason, and a publication that does not exist. Deleting a publication that has an attempt fails on `publication_attempt_publication_id_fkey`. Read from `pg_constraint`, that is the attempt's only foreign key, with no delete action.
- `the_schema_lists_every_endpoint` lists the new path. `the_schema_publishes_the_error_codes_and_states_as_fixed_sets` checks `AttemptResult`.
- A throwaway test printed the schema of the new path and its types. `result`, `reason`, and `ended_at` were required and nullable, and the path answered 200, 401, 404, 500, and 503.
- Each of these changes, made by a script and reverted, fails a test:
  - The organization filter removed from the attempts query, or from the publication lookup: `another_organizations_attempts_are_not_found`.
  - The publication lookup skipped: the same test, where a publication that does not exist answers an empty list.
  - The U+0000 check removed: the same test, with 500 `internal`.
  - Oldest first: `the_list_shows_each_attempt_newest_first`.
  - The id check, the result check, the ended check, or the reason check removed, or the reason check loosened to let `outcome_unknown` through: `the_table_refuses_an_attempt_it_cannot_explain`.
  - The publication reference removed, or set to cascade, or an `organization_id` added that cascades: the same test.

Asserted, not verified:

- The API Docker image embeds the new migration. The image was not built, as for #13–#17.
- `api serve` was not run by hand. The schema was read from the same router through a unit test.
