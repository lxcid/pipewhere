# Plan 00003 — Scheduling and recovery

The build runs as one small PR per issue, as 00002's did. Each issue carries its own scope and dependencies. This plan records what each one built as it lands.

## Shape

- API's library holds the run's steps, so Worker writes through the same application layer as API, per infrastructure D4. Worker's crate depends on `pipewhere-api` and keeps only Restate, the poller, and its commands.
- Worker's poller finds due work in PostgreSQL, per D5, and sends each publication's run to Restate.
- A run is a virtual object invocation whose journal is a sequence of PostgreSQL steps, per D6. D7 and D8 say what the steps do.
- `worker recover` re-drives runs in flight, per D9.
- API gains the tolerance and deadline (D11), the overdue list, and the scheduler's stall (D10).

## Steps

1. **#26 Tolerance and deadline on a publication.** `tolerance_seconds` on create and edit, `deadline` in every publication response. The migration gives existing publications 1,800 seconds. D1, D2, D11. Built in PR #35.
2. **#27 An organization's default tolerance.** `pipewhere.organization_setting`, `GET` and `PATCH .../settings`. Create reads the default. D1, D2, D11. Built in PR #36.
3. **#28 The overdue list.** `?overdue=true` on the publication list. D10, D15. Depends on #26 only for the response shape. Built in PR #37.
4. **#29 The run's steps in API's library.** The claim with its attempt, the commit claim with its deadline check, the adapter seam with no adapter, result recording, and the `cancelled` attempt result. Tested against PostgreSQL without Restate. D7, D8. Depends on #26. Built in PR #38.
5. **#30 Worker's first handler and the poller.** The `Publication` virtual object, Worker's database connection, the poller and its partial index, Compose changes, and a Restate test harness. Carries infrastructure D14's upgrade test and D15's dependency. D5, D6. Depends on #29. Built in PR #39.
6. **#31 The scheduler's stall.** `pipewhere.scheduler_tick`, written by the poller, and `GET .../scheduler`. D10. Depends on #30.
7. **#32 `worker recover`.** Tested after losing Restate, and while the original invocation is live. README runbook. D9. Depends on #30.
8. **#33 Channel status at the claim, and on a waiting publication.** D3 and D4, and the intent's fourth outcome. Blocked until 00004 gives a channel a readable status, per D12.

## Verification approach

Each issue runs, at its head:

- `cargo test -p pipewhere-api` and, from #30 on, `cargo test -p pipewhere-worker`
- `pnpm --filter @pipewhere/auth test`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all --check`

The tests need `pipewhere-test-postgres`, started by `pnpm --filter @pipewhere/auth test:db`. From #30 on they also need a Restate server.

Concurrency and recovery are tested with a transaction held open, or with real requests racing, as #16 and #17 did:

- two claims of one publication
- a cancel racing the commit claim
- a re-drive while the original invocation is live
- a step interrupted after the fake adapter accepted, which must end `outcome_unknown` with one call to the adapter
- the step code changed under a run held inside a step, for infrastructure D14

## #26 — what was built

- The migration `20261103133134_publication_tolerance.sql` adds `tolerance_seconds` to `pipewhere.publication`, per D1 and D11:
  - It is an `integer`, checked from 300 to 86,400 by `publication_tolerance_seconds_check`.
  - Existing publications get 1,800, per 00002 D12.
  - The column's default is dropped after the backfill. A writer that leaves the tolerance out fails on `NOT NULL`.
- `apps/api/src/publications.rs`:
  - Create takes an optional `tolerance_seconds`. When the request names none it used 1,800 until #27 replaced that with the organization's default.
  - Edit takes an optional `tolerance_seconds`, in the same conditional update as the body and planned time, so in `scheduled` and `failed` only.
  - Retry does not touch the tolerance, so it keeps it.
  - `checked_tolerance`, now in `settings.rs`, refuses a value outside 300 to 86,400 with 422 `invalid_body`, on create and edit.
  - Every publication response shows `tolerance_seconds` and `deadline`. The `columns!` macro computes the deadline as `planned_at + tolerance_seconds * interval '1 second'`. Nothing stores it.
  - Create's request hash gains `tolerance_seconds` when the request sends one, per D13. A request without one hashes as it did before #26, so keys stored then still match, per 00002 D5. Retry's hash is unchanged.
- The schema publishes `tolerance_seconds` with `minimum` 300 and `maximum` 86400 on `Publication`, `PublicationInput`, and `PublicationEdit`, and `deadline` as `date-time`. Both are required on `Publication`.

Choices made in the build, within the spec:

- The deadline adds a whole number of seconds. An interval of seconds holds no days, so adding it does not depend on the session's time zone.
- `tolerance_seconds: null` is treated as omitted, per D13. Create then uses the default, and edit keeps the current tolerance.
- Tolerated: a planned time in the last day of 9999 can carry its deadline past the published format, per D14.
- No index added. The deadline is computed per row read.

Verified:

- `cargo test -p pipewhere-api`: 6 unit tests and 50 integration tests pass. `a_publication_records_its_tolerance` is new in `tests/publications.rs`.
- `pnpm --filter @pipewhere/auth test`: 20 tests pass.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check`: clean.
- `tests/publications.rs` and `tests/publication_transitions.rs` passed 3 runs in a row.
- `a_member_creates_lists_and_reads_publications` reads `tolerance_seconds: 1800` and `deadline: 2026-12-01T09:30:00Z` from create, the single read, and the list.
- `a_publication_records_its_tolerance` creates with 300 and 86,400, the two bounds, and reads deadlines 5 minutes and 24 hours after the planned time. A request to publish now gets 1,800 and a deadline 30 minutes after its planned time.
- `a_body_api_cannot_read_is_refused` refuses 299, 86,401, 0, 4,294,967,296, and `"30m"` on create with 422 `invalid_body`. `an_edit_changes_only_this_channels_copy` refuses 299, 86,401, −1,800, 1,800.5, and `"1800"` on edit. Its two allowed tolerance edits, 300 and 86,400, change only `tolerance_seconds` and `deadline`. Its planned-time edit moves the deadline with it.
- `each_action_is_allowed_only_in_the_states_d3_lists` gains a tolerance-only edit, `edit tolerance`. It succeeds in `scheduled` and `failed`, and answers 409 `state_conflict` in the other five states, changing nothing. `resolving_records_what_a_person_found` and `another_organizations_publication_is_not_found` run the same edit through their loops.
- `a_repeated_retry_returns_the_publication_without_retrying_again` retries a publication with a 600-second tolerance. The retry keeps 600, and the deadline moves to the new planned time plus 10 minutes.
- `a_repeated_request_returns_the_first_result`:
  - It reads the hash stored for a create without a tolerance. The hash equals SHA-256 of the five-element request that main hashed before #26.
  - It sends the key again with `tolerance_seconds: null`, and gets the first result.
  - It sends the key again with `tolerance_seconds: 1800` added, and gets 422 `idempotency_key_reused`.
  - It creates with a new key and a tolerance of 300. Repeating with 300 gets the first result, and repeating with 600 gets 422 `idempotency_key_reused`.
- `an_edit_changes_only_this_channels_copy` edits with `tolerance_seconds: null`, and the publication is unchanged.
- `the_tables_refuse_bad_rows` shows the table refusing 299 and 86,401 (`publication_tolerance_seconds_check`), and an insert that leaves the column out (`23502`, not null).
- `the_schema_publishes_the_error_codes_and_states_as_fixed_sets` checks the bounds on all three schemas, `deadline`'s format, and which fields are required. `the_schema_lists_every_endpoint` is unchanged and passes.
- The backfill, by a throwaway test against `pipewhere-test-postgres`, deleted afterwards:
  1. `ALTER TABLE pipewhere.publication DROP COLUMN tolerance_seconds`, to return the table to its shape before #26.
  2. Insert a publication without the column.
  3. Run the migration's two `ALTER TABLE` statements.
  4. The row's `tolerance_seconds` read 1,800. `information_schema.columns` showed a null `column_default` for the column.
- D14's year-10000 deadline, by a throwaway test, deleted afterwards. A create with `"planned_at": "9999-12-31T23:59:59Z"` and no tolerance answered `"deadline": "+10000-01-01T00:29:59Z"`.
- Each of these changes, made by a Python script outside the repository, fails a test. The script copied the file aside, made one change, ran the tests, and restored the file.
  - API's range check skipped: `a_body_api_cannot_read_is_refused` and `an_edit_changes_only_this_channels_copy`, where the table's check answers 500.
  - The table's check removed, or either bound loosened by one: `the_tables_refuse_bad_rows`.
  - The column default kept: the same test.
  - The edit's state guard bypassed when the edit changes only the tolerance: `each_action_is_allowed_only_in_the_states_d3_lists` and `resolving_records_what_a_person_found`.
  - The tolerance dropped from the request hash: `a_repeated_request_returns_the_first_result`.
  - An omitted tolerance hashed as `null` after the other fields: the same test.
  - The hash recording only that a tolerance was sent, not its value: the same test.
  - The deadline subtracting the tolerance, or counting it in minutes: four tests, among them `a_member_creates_lists_and_reads_publications`.
  - The deadline set to 30 minutes, whatever the tolerance: `a_publication_records_its_tolerance`, `an_edit_changes_only_this_channels_copy`, and the retry test.
  - Create storing 1,800 whatever the request sent: `a_publication_records_its_tolerance`.
  - Retry resetting the tolerance to 1,800: `a_repeated_retry_returns_the_publication_without_retrying_again`.

- The running stack, at `efa2acf`. `docker compose up -d --build --wait` built the images, and `api-migrate` applied the migration to the existing local database. The command exits 1, because `--wait` counts `worker-register` exiting as a failure, although it exited 0. Then, by hand against API on port 3002:
  - `/openapi.json` publishes the tolerance's bounds and `deadline`.
  - Create with no tolerance answered 1,800 and a deadline 30 minutes later. Create with 300 answered a deadline 5 minutes later.
  - The same key and tolerance answered the first result. The same key with 600 answered 422 `idempotency_key_reused`. A tolerance of 299 answered 422 `invalid_body`.
  - An edit to 3,600 moved the deadline. An edit after cancelling answered 409 `state_conflict`.
  - The list and the single read show both fields.

Not verified:

- Signing in through Web with the account that check created. Web has no publication pages yet.

## #27 — what was built

- The migration `20261106100000_organization_setting.sql` adds `pipewhere.organization_setting`, per D11:
  - `organization_id` is the primary key and references `auth.organization`. It does not cascade, per 00001 D2.
  - `tolerance_seconds` is an `integer`, checked from 300 to 86,400 by `organization_setting_tolerance_seconds_check`.
- `apps/api/src/settings.rs`:
  - `GET /v1/orgs/{organization_id}/settings` answers `{ "tolerance_seconds": ... }`. An organization without a row answers 1,800, and reading writes no row.
  - `PATCH` on the same path inserts or updates the row, and answers the same shape. It is open to every member and key, per D11.
  - `default_tolerance` is the one place that says no row means 1,800. Both `GET` and create read it.
  - `checked_tolerance` moved here from `publications.rs`, so all three writers share it. It now takes a value rather than an `Option`.
- `apps/api/src/publications.rs`: when the request names no tolerance, create reads the default inside its transaction, after the key is claimed. A repeated request therefore never reads it. The request hash is unchanged, per D13. Edit and retry do not read the default.
- The schema publishes both new types, with `minimum` 300 and `maximum` 86400.

Choices made in the build, within the spec:

- `tolerance_seconds` is required on `PATCH`, as `body` is on content's. `{}` and `null` answer 422 `invalid_body`. It is the only setting, so an edit without it would do nothing. Making it optional when a second setting arrives breaks no client.
- Unknown fields answer 422, as on the publication endpoints.
- The response has no `organization_id`. The settings have no id of their own, and the path names the organization.
- Nothing removes a row. Going back to 30 minutes means setting 1,800.
- A create racing a `PATCH` takes either value. Each was the organization's default during the request.
- `20261103133134_publication_tolerance.sql` still says the range is repeated in `src/publications.rs`. That stays true of the schema attributes there, and its checksum stops the comment being edited.

Verified, at `50219c4`:

- `cargo test -p pipewhere-api`: 6 unit tests and 56 integration tests pass. `tests/settings.rs` is new, with 5 tests.
- `pnpm --filter @pipewhere/auth test`: 20 tests pass.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check`: clean.
- `tests/settings.rs`, `tests/publications.rs`, and `tests/publication_transitions.rs` passed 3 runs in a row.
- `a_member_reads_and_changes_the_default_tolerance` reads 1,800 with no row, then changes the default to 600, 300, and 86,400. One row exists after each change.
- `any_member_or_key_changes_the_default_tolerance` reads and changes it as a `member` and as a key.
- `another_organizations_settings_are_not_found`:
  - Changing one organization's default leaves another's at 1,800.
  - A user who is not a member, and another organization's key, get 404 on read and change.
  - No token gets 401. An outsider sending a bad body gets 404, not `invalid_body`.
- `a_body_api_cannot_read_is_refused` in `tests/settings.rs` refuses each of these, and the stored default stays 600:
  - 400: not JSON. 415: no content type. 413: over 2 MB.
  - 422: `{}`, `null`, an unknown field, `"900"`, 900.5, 4,294,967,296, 299, 86,401, 0, and −1,800.
- `the_table_refuses_bad_rows` shows the table refusing 299 and 86,401, an unknown organization, and a second row for one organization. An organization with a row cannot be deleted.
- `a_publication_takes_its_organizations_default_when_it_is_created`, with the default at 600 and then 3,600:
  - An omitted tolerance and `null` take 600, and an explicit 300 overrides it. Another organization still gets 1,800.
  - After the change, the first publication reads as it did. A repeat of its request answers that publication, per D13. A new create takes 3,600.
  - An edit that names no tolerance keeps 600.
- `a_repeated_retry_returns_the_publication_without_retrying_again` retries a 600-second publication with the default at 3,600. The retry keeps 600.
- Each of these changes, made at `88bcbfa` by a Python script outside the repository, fails a test. The later commit changes only schema descriptions and a test comment. The script copied the file aside, made one change, ran the tests, and restored the file.
  - API's range check skipped, or either bound loosened by one: `a_body_api_cannot_read_is_refused` and `an_edit_changes_only_this_channels_copy`.
  - The table's check removed, either bound loosened by one, or the foreign key removed: `the_table_refuses_bad_rows`.
  - The default's lookup ignoring the organization: `another_organizations_settings_are_not_found` and the create test.
  - The 1,800 constant changed: five tests, among them `a_member_reads_and_changes_the_default_tolerance`.
  - The upsert keeping the old value: three tests.
  - Unknown fields allowed: `a_body_api_cannot_read_is_refused`.
  - Create ignoring the default: the create test.
  - Create ignoring the request's tolerance: that test and `a_publication_records_its_tolerance`.
  - Create hashing the default it fills in: that test and `a_repeated_request_returns_the_first_result`.
  - Changing the default also rewriting publications: that test and the retry test.
  - Retry taking the default: the retry test.
  - An edit without a tolerance taking the default: the create test.
  - Either new schema losing its bounds: `the_schema_publishes_the_error_codes_and_states_as_fixed_sets`.

Not verified:

- The running stack. The local Compose stack still runs #26's build, and this migration has not been applied to its database. Applying it there fixes the migration's checksum for that database.
- Two first `PATCH`es forced to interleave. They rely on `ON CONFLICT`. Only repeated `PATCH`es in sequence are tested. A probe in review sent 25 first `PATCH`es at once: all answered 200 and left one row.

## #28 — what was built

- `apps/api/src/publications.rs`: `GET /v1/orgs/{organization_id}/publications?overdue=true` adds one condition to the list's query, `state = 'scheduled' AND planned_at < now()`, per D10. The order, the page size, and the cursor are the list's own, per 00002 D14.
- `false`, or leaving the parameter out, lists every publication. Any other value answers 422 `invalid_query`, per D15. `apps/api/src/error.rs` adds that code.
- The schema publishes `overdue` as an optional boolean query parameter, and the 422. It says to send the same `overdue` with `cursor`, and that work about to start is listed.

Choices made in the build, within the spec:

- "Newest first" is by id, as 00002 D14 orders the list. It is not by planned time, so the most urgent publication can sit on a later page.
- The cursor does not carry the filter. A client that sends only `?cursor=` on its second page gets the unfiltered list. The schema says to repeat `overdue`. Carrying the filter would make the cursor more than an id, and only an organization with over 100 overdue publications reaches this.
- A publication is listed as soon as its planned time passes. Once #30's poller runs, a healthy publication stays listed until its run's first step claims it. A tick only sends the run, and sends at most 100, per D5. A backlog can therefore stay listed for longer than one tick. How long is unmeasured until #30.
- Until #30, nothing claims a publication, so everything past its time stays listed until it is edited or cancelled.
- No index added. The filter scans `pipewhere.publication` across every organization, because the table's only index is its primary key. D14's estimate of 10 publications a day reaches 1,000,000 rows in about 270 years for one organization, but in 100 days for 1,000. That is still acceptable now:
  - The plain list already has the same whole-table cost for an organization with fewer than 101 publications.
  - No production target or client depends on this latency yet.
  - #30's partial index on `scheduled` publications served this query in a review probe. See below.

Verified, at `79acce4`:

- `cargo test -p pipewhere-api`: 7 unit tests and 58 integration tests pass. `the_overdue_list_shows_what_passed_its_planned_time_without_starting`, `the_overdue_list_pages_by_cursor`, and `the_schema_publishes_the_overdue_filter` are new.
- `pnpm --filter @pipewhere/auth test`: 20 tests pass.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check`: clean.
- `tests/publications.rs` passed 3 runs in a row.
- `the_overdue_list_shows_what_passed_its_planned_time_without_starting`:
  - It lists a `scheduled` publication 1 minute past its planned time, and one 2 days past, which is beyond its deadline. The newer id, with the older time, comes first.
  - It does not list a `scheduled` publication planned an hour ahead, one in each of the other six states planned in the past, or another organization's overdue publication.
  - A request to publish now is listed on the next read.
  - `overdue=false` answers exactly what the list answers without the parameter.
  - These answer 422 `invalid_query`: `True`, `False`, `TRUE`, `1`, `0`, `yes`, `true` with a trailing space, an empty value, no value, NUL, invalid UTF-8, and a bad value with a NUL cursor.
  - A user who is not a member gets 404, and no token gets 401, with a good value and a bad one.
- `the_overdue_list_pages_by_cursor` has 150 overdue publications alternating with 151 that are not, the oldest of them not overdue. The first page holds 100 overdue, and `next_cursor` is the last of them. The second holds the other 50, and `next_cursor` is null.
- Each of these changes, made by a Python script outside the repository, fails a test. The script copied the file aside, made one change, ran the unit tests and `tests/publications.rs`, and restored the file.
  - The state condition dropped: the first new test.
  - The time condition dropped, or its comparison reversed: both new tests.
  - The deadline compared instead of the planned time: both new tests.
  - Work past its deadline hidden: the first new test.
  - The filter on when left out or `false`: that test and `a_member_creates_lists_and_reads_publications`.
  - `false` turning the filter on, or being refused: the first new test.
  - `true` turning the filter off: both new tests.
  - Other values listing everything, or turning the filter on: the first new test.
  - The value compared without case, or a NUL cursor skipping the refusal: the first new test.
  - The refusal answering 400, or `invalid_body`: the first new test.
  - The organization filter dropped: that test and `another_organizations_resources_are_not_found`.
  - The cursor ignored under the filter: `the_overdue_list_pages_by_cursor`.
  - Ordered by planned time, or oldest first: the first new test and others.
  - The schema's parameter or its 422 removed: `the_schema_publishes_the_overdue_filter`.
- The scan's cost, by throwaway tests, deleted afterwards. All ran against `pipewhere-test-postgres` on a laptop, with API in a debug build.
  - One organization held 1,000,000 `cancelled` publications with 500-character bodies, and nothing was overdue. 5 requests with `overdue=true` took 334, 70, 251, 238, and 241 ms. The same requests without it took 17, 16, 10, 6, and 5 ms.
  - `EXPLAIN ANALYZE` of the query with its values written in chose a parallel sequential scan. It took 10 ms at 100,000 rows and 64 ms at 1,000,000.

Found by a review probe at `9994685`, not repeated here. The filter's query has not changed since:

- An organization with 3 publications, in a table of 1,000,003 rows, scanned the whole table. Its requests took 232–309 ms.
- The time is spent in PostgreSQL, not the debug build. `EXPLAIN ANALYZE` of the generic plan took 226–271 ms. The probe inferred from matching timings, without capturing a live request's plan, that steady requests use that plan rather than the 64 ms one above.
- The plain list, for the same organization, took 46–513 ms.
- With #30's partial index present, requests on fresh connections took 1–2 ms after the first. Connections warmed before the index was created stayed at 243–308 ms for 15 requests. #30 should not judge the index by a connection opened before it existed.
- `overdue=true&overdue=false` answered the whole list. A publication planned 2 seconds ahead was not listed, then was listed 2.5 seconds later.

Not verified:

- The running stack. The local Compose stack still runs #26's build.
- `<` differing from `<=`, which a real request cannot observe.
- Whether generated clients, PHP, or .NET send `true`. D15 refuses anything else, so a client that sends another spelling finds out on its first request.
- The one failure seen in this build. In one full run, three tests in `tests/contents.rs` panicked because Auth did not answer `/health` within the test harness's 10 seconds. That file and the harness did not change. The cause is not known, and later runs of that file passed.

## #29 — what was built

- `apps/api/src/runs.rs` holds `step(db, adapter, publication_id)`, which reads the publication's state and takes the one step that state calls for. It answers the state the run goes on from. The run takes another step only from `preparing`. Nothing calls it yet, because #30 adds the handler.

| State found | What the step does |
| --- | --- |
| `scheduled` | The claim, per D7. Within the deadline: `preparing` with a running attempt. Past it: `failed`, with an attempt that starts and ends at the same time. |
| `preparing` | The commit, per D8: the commit claim, the adapter, then the result. Past the deadline: `failed`, and the adapter is not called. |
| `publishing` | `outcome_unknown`, and the attempt ends with it. The adapter is not called. |
| `cancelled` | Any running attempt ends `cancelled`, with no reason. |
| `published`, `failed`, `outcome_unknown` | Nothing. |

- The commit claim locks the row and reads its state in one transaction. A state other than `preparing` goes back to this table. A publication past its deadline fails in that same transaction.
- The adapter seam is the `Adapter` trait. `commit` takes the provider and body, and answers `Published`, `Refused`, or `Ambiguous`. `NoAdapter` is what production runs until 00004. It refuses with "Pipewhere cannot publish to Mastodon yet."
- The migration `20261107000000_attempt_cancelled.sql` adds `cancelled` to the attempt's result check, and lets it have no reason. `AttemptResult` gains the value, with 00002 D8's cost.
- D16, D17, and D18 record what these steps added to the spec. The operator approved all three:
  - D16: a claim waits for the planned time.
  - D17: a claim's loser, and an answer that finds its attempt ended, each answer the state the step found or claimed. It also tolerates two cases: a claim's winner dying, and a stalled execution acting on a later run's state.
  - D18: an adapter's post reference must meet 00002 D18's rules.

Choices made in the build, within the spec:

- The commit claim takes the row's lock first, then checks the deadline against `clock_timestamp()`, so D2's check still sits in the update that claims the commit. A waiting update does not check its conditions again when the lock's owner rolls back, so the check would otherwise use a time from before the wait. Changing `now()` to `clock_timestamp()` alone does not fix this.
- A missed deadline fails under that same lock. Reading the state after releasing it could find a later run in `preparing`, and fail that run with a false reason.
- The commit claim returns the provider and body, so the adapter gets what the claim committed. It also returns the running attempt's id.
- A provider's answer applies only while that attempt is still running. The publication being `publishing` does not show this, because a person can resolve and retry it while an old execution waits for its answer. An answer that finds its attempt ended changes nothing, and the step answers `publishing`, per D17, so an old handler stops.
- A commit claim finds no row when no attempt is running, and the adapter is not called. Nothing could record what the provider did. The step then fails with an error, as below.
- A result that cannot be recorded logs the provider's answer at `error`, including the remote id. That log is the only record of the id.
- A move into `failed`, `outcome_unknown`, or `published` with no running attempt to end is an error and rolls back, per 00002 D19. No path reaches that state. A hand-edited row would make the invocation keep failing.

Tolerated:

- A provider answer holding U+0000 cannot be stored, so recording it fails, and the next step records `outcome_unknown`. 00004's adapters own what they send.
- A claim's winner dying, and a stalled execution acting on a later run's state, are D17's two tolerated cases.

Left for #30: the handler's loop, `Deserialize` on `PublicationState` for the journal, and Worker's dependency on this crate.

Left for 00004: D19, a time limit on an adapter's provider call, because an old call can post after a person resolves and retries.

Verified, at `19b849f`:

- `cargo test -p pipewhere-api`: 7 unit tests and 83 integration tests pass. `tests/publication_runs.rs` is new, with 25 tests.
- `pnpm --filter @pipewhere/auth test`: 20 tests pass.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check`: clean.
- `tests/publication_runs.rs`, `tests/publication_attempts.rs`, and `tests/publication_transitions.rs` passed 3 runs in a row.
- Two claims racing: a held transaction keeps both waiting at their updates, checked in `pg_stat_activity`. Once it commits, one claim answers `preparing` and the other `scheduled`, with one attempt.
- The claim's one transaction: with the attempt table locked, the claim waits at its insert, and the publication still reads `scheduled`.
- A cancel racing the commit claim:
  - A cancel held open and then committed: the step answers `cancelled`, the attempt ends `cancelled`, and the adapter is not called. If it rolls back, the step publishes.
  - A commit claim already committed, with the adapter held mid-call: the publication reads `publishing`, and both cancel and edit answer 409.
  - 25 rounds of a real cancel request against a real step agree on who won, in the state, the attempt, and the number of adapter calls. This is weak evidence for the cancel's side. In a review probe at `bd1d440`, the cancel won 5, 2, and 1 of 25 rounds in 3 runs, and the test does not require it to win any. The held tests are what force each order.
- A commit claim that waits past the deadline: with 3 seconds left, a held cancel keeps the step waiting until the deadline passes, then rolls back. The step answers `failed`, without an adapter call.
- A cancel held open while the step fails a publication past its deadline: the outcome follows the cancel's commit or rollback.
- A step that loses its move: another transaction moves `publishing` to `published` while the step waits. The step answers `published`, and changes nothing.
- An edit moving the time later, racing a claim: the claim loses, and no attempt is written.
- An interrupted commit: the step's task is aborted while the fake adapter holds the call. The next step records `outcome_unknown`, and the adapter's call count stays 1. A later step changes nothing.
- A late answer, in two cases:
  - A second step records `outcome_unknown` while the first holds the adapter. When the first gets `Published`, it changes nothing, leaves no remote id, and answers `publishing`.
  - A person then resolves and retries, and a new run holds a second adapter. The old refusal arrives and changes nothing. The new run still publishes, with its remote id, and each attempt keeps its own result.
- A commit past its deadline fails without an adapter call. A commit whose deadline passes during the adapter's call still records `published`.
- Ending a run or cancelling one leaves another publication's running attempt running.
- The claim's bounds, with tolerances of 300, 600, and 86,400 seconds, on each side of the deadline.
- A retry after a late failure runs again, and leaves 2 attempts.
- Reading through the API: a running attempt, a `cancelled` attempt, and a `published` publication's remote id.
- Each of these changes, made by a Python script outside the repository, fails a test. The script copied the file aside, made one change, ran the unit tests, `tests/publication_runs.rs`, and `tests/publication_attempts.rs`, and restored the file. 24 ran at `3d28600`, on the commit path and the ending it shares. The rest ran at `389adf5`, on code that has not changed since.
  - At `19b849f`, failing a missed deadline after the commit claim's transaction commits. It failed `a_cancel_queued_behind_a_missed_deadline_finds_it_failed` in 3 of 3 runs, and no other test in `tests/publication_runs.rs`. That test passes at this head.
  - At `3d28600`, the commit claim: ignoring the state or the deadline; its deadline reversed; leaving the state at `preparing`; not taking the lock; reading the transaction's time; not committing; needing no running attempt; taking another publication's attempt; a missed deadline moving nothing; failing a state other than `preparing`.
  - At `3d28600`, an answer: claiming its own ending when it lost; answering the current state; ending what it finds; ignoring its own attempt.
  - At `3d28600`, the ending: an interrupted commit becoming `failed`; not reading the state again after losing a race; ignoring the state; a missing attempt passing; rewriting ended attempts; ending other publications' attempts; adding a reason where there is none; two transactions in place of one; not committing.
  - At `389adf5`, the claim: ignoring the planned time, the state, or the tolerance; its deadline reversed; always `preparing`; always `failed`; a loser continuing the run; a late failure ending later than it started, or leaving its attempt running; not committing; two transactions in place of one.
  - At `389adf5`, the result: refused and ambiguous swapped; the address dropped; a published attempt recorded as failed; the provider's reason replaced; the adapter called twice; an interrupted commit calling the adapter again.
  - At `389adf5`, a cancelled run: leaving its attempt running, ending it with another result, rewriting ended attempts, or ending other publications' attempts.
  - At `389adf5`, `NoAdapter`: naming the wrong provider, or answering ambiguous.
  - At `389adf5`, the table: refusing the new result; requiring a reason for it; allowing any result; dropping the reason for `failed` and `outcome_unknown`.
  - At `389adf5`, the schema publishing another name for the new result.

Not verified:

- Restate. Nothing runs a step through a handler until #30. That includes the Send bound on a real `ctx.run`, although the tests run `step` inside `tokio::spawn`, which needs `Send`.
- The running stack. The local Compose stack still runs #26's build, and neither this migration nor #27's has been applied to its database. No request reaches the new code there.
- The log line for a result that cannot be recorded. No test reads the log. The late-answer tests run that path.
- A real provider. The results other than `NoAdapter`'s refusal come from a fake adapter.
- `<=` differing from `<` at either time check, which a test cannot observe.
- The deadline being rechecked between the commit claim's update and its commit. That window is one round trip, and the claim then counts as started.
- Why Auth sometimes misses the harness's 10-second `/health` wait. Review runs saw this four times: once in `tests/publications.rs`, twice in `tests/publication_attempts.rs`, and once in `tests/publication_runs.rs`. One, lasting 10.85 seconds, saved no message, so it is only probably the same failure. The others show it during setup, before the test's own code runs. Two full reruns with fewer test threads passed. Load was not measured, the harness is unchanged from `main`, and the cause is still not known. A repeat run can therefore fail for a reason other than the code.

## #30 — what was built

- `apps/worker/src/publication.rs` holds the `Publication` virtual object, generic over `Adapter`, per D6. Its `run` handler takes `step` entries until one answers anything but `preparing`. Production binds `NoAdapter`.
- `apps/worker/src/poller.rs` holds the poller, per D5. It runs inside `worker serve`, beside the Restate endpoint, so it stops with the process. A failed tick is logged, and the next one tries again.
- `apps/worker/src/lib.rs` holds `register`, moved from `main.rs` so the tests register the same way.
- The migration `20261107120000_publication_due.sql` adds the partial index `publication_due`.
- `compose.yaml` gives Worker `DATABASE_URL` and `RESTATE_INGRESS_URL`, and makes it wait for `api-migrate`. `api` now waits for `worker-register` to succeed, per 00001 D15.
- `PublicationState` gains `Deserialize`, for the journal.
- `apps/worker/tests/publication_runs.rs` starts the Restate image `compose.yaml` pins, with Docker, one container per test. It reuses API's test harness through `#[path]`, so publications are created and read through the real API.

Choices made in the build, within the spec:

- A tick stops at its first failed send. The next tick starts again from the oldest work. Going on would wait up to 10 seconds per send when Restate is down.
- `Publication` sets its retry limit to 4,294,967,295 attempts. Restate 1.7.13's default is 70 attempts, about 63 minutes, and then it pauses the invocation. A paused invocation stays paused after the failure clears, so its publication would never reach its deadline check. At Restate's limit of one attempt a minute, the new limit is not reached.
- A send has a 10-second timeout, because a Restate that never answers would otherwise stop every later tick without an error.
- The D14 tests run a Worker on its own Tokio runtime, and shut that runtime down to stop it mid-step. `serve_with_cancel` would wait for the open invocation instead. They change the code by starting a Worker with another adapter on the same port.

Tolerated:

- A publication whose claim keeps failing while the poller keeps working gets another queued invocation every tick. That is 17,280 a day for one publication, and up to 100 a tick across all of them. D5 calls duplicate sends harmless. Once the failure clears, each queued run reads the state and stops. The test covers 5 duplicates. What a long queue costs Restate is unmeasured.
- A run sent for a publication that does not exist retries until the row appears. The poller sends only ids it read from the table, so this needs a send by hand.

Verified:

- `cargo test -p pipewhere-worker`: 9 integration tests pass, in 3 runs in a row.
- `cargo test -p pipewhere-api`: 7 unit tests and 83 integration tests pass, at `1c8a057`.
- `pnpm --filter @pipewhere/auth test`: 20 tests pass, at `1c8a057`.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check`: clean.
- `the_poller_runs_due_work_and_leaves_the_rest_waiting`: through the API, a due publication fails with the no-adapter reason, a late one fails as late, and a future one stays `scheduled` with no attempt.
- `sending_a_run_again_publishes_once`: 5 sends leave one attempt, one adapter call, and the remote id.
- `a_run_sent_before_the_planned_time_changes_nothing`, per D16.
- `a_step_that_fails_runs_again`: a step that errors stays with Restate's retries, and publishes once it can. Worker's tests run Restate with a default limit of 2 attempts, and this test waits for 3 retries. It timed out before `Publication` set its own limit.
- `a_tick_fails_until_restate_can_take_the_run`: a failed query, a closed port, and a Restate with no deployment each fail the tick.
- `a_tick_takes_the_oldest_due_work_first`: the limit, the order with and without the index, the state, and the time.
- `registering_again_picks_up_a_new_handler`: an installation moving from no handlers to this one, which needs `force`.
- `a_commit_interrupted_by_an_upgrade_never_repeats`, for 00001 D14. A Worker stops while its adapter holds the call. The journal then holds exactly `Input`, `step`, its completion, and `step`. The new Worker records `outcome_unknown` and calls no adapter.
- `a_run_held_before_its_commit_continues_on_the_new_code`, for 00001 D14. A Worker stops after its first step is recorded. The new Worker writes no second attempt, and publishes with its own adapter.
- A script outside the repository broke the handler, the poller, and `force` in 14 ways, one at a time. Each now fails a test. Four survived at first, and the tests above were extended to catch them.
- The running stack, at `1c8a057`, in a throwaway Compose project with its own volumes, since removed. Web was moved to port 3100, because another process held 3000.
  - `docker compose up -d --build --wait` exits 0 and registers one service, per 00001 D15.
  - Through the API, a due publication left `scheduled` within about a second and failed with the no-adapter reason. A late one failed as late. A future one stayed `scheduled`.
  - With sequential scans off, `EXPLAIN` of the poller's query uses `publication_due`.
  - After `docker compose up -d --build --wait worker worker-register`, a new due publication still ran. That command exits 1 with `--wait`, per 00001 D14's cost, and the README's form omits `--wait`.
  - With Restate stopped, a tick logged an error and the publication stayed `scheduled`. Once Restate started again, it ran.
  - With `RESTATE_ADMIN_URL` pointing nowhere, `docker compose up -d api` exits 1, and API does not start.

Not verified:

- A real provider. Publishing comes from a fake adapter.
- A real change of Worker's binary under a run. The D14 tests change the adapter on a running test process. The stack's rebuild had no run in flight.
- A run failing for longer than Restate's default limit of about 63 minutes. The test lowers that default instead of waiting.
- Losing Restate's volume. #32 tests this.
- The tests' setup through a real `psql` on the host, which this machine does not have. They ran with a script that runs `psql` 18.6 in the test server's container.

## #31 — scheduler visibility

Draft PR [#40](https://github.com/lxcid/pipewhere/pull/40), for approved D10. The pipeline remains in progress.

- API's migration adds `pipewhere.scheduler_tick`, initially empty. A boolean primary key and check permit only one row, and its completion time cannot be null.
- `scheduler::record_tick` in API's library records PostgreSQL's clock after the poller's query and every send succeed. An empty tick counts. A failed or partially sent tick leaves the last completion unchanged. A failure to record the tick fails the tick.
- `GET /v1/orgs/{organization_id}/scheduler` uses the existing membership/key authorization. It returns `last_completed_at` and `stalled`. Before the first completion it returns null and true. At 60 seconds without a completion it reports stalled. The clock and the comparison both come from PostgreSQL.
- The status is installation-wide, as D10 requires. A healthy tick does not establish that Restate can execute accepted work; the overdue publication list remains necessary.
- The generated OpenAPI includes the endpoint, response and bearer authentication. No handler journal, publication state or provider call changes.

Verification on the isolated `feat/31-scheduler-health` checkout, Rust 1.95.0 and Node 26.10.0:

- The generated-route test and the first real API behavior test failed before implementation; the latter returned 404.
- `cargo test -p pipewhere-api --locked -- --test-threads=1`: the existing integration suite and four new scheduler tests pass against real Auth and a dedicated PostgreSQL 18.6 container. `cargo test -p pipewhere-api --lib --locked` also passes all 8 unit tests, including the generated schema checks. They cover no tick, recent and stale ticks, timestamp serialization, member and key reads, wrong-organization and anonymous refusals, removed membership and the table constraints.
- `cargo test -p pipewhere-worker --locked -- --test-threads=1`: all 10 Worker integration tests pass through Restate 1.7.13. The tick tests cover empty and successful batches, query/send failures and a batch whose second send fails.
- `pnpm --filter @pipewhere/auth test` (the package script was invoked directly as `node --test --test-concurrency=1 'test/**/*.test.ts'` from `apps/auth`): all 20 tests pass. Test accounts and secrets are dummy fixtures. A test-only wrapper runs `psql` in the dedicated container because the host has no `psql`.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo build --workspace --locked`, `cargo fmt --all --check` and `git diff --check` pass. This repository has no hosted CI. Independent Claude/Codex review receipts and their reconciliation are in the draft PR.

Not verified: a whole-stack Compose restart with the new migration, a deployed installation, or any provider. This slice adds a read-only API status, not the founder feedback/approval/slot UI or a publishing adapter.

## Not yet built

#32–#33. #33 is blocked on 00004, per D12.
