# Plan 00003 — Scheduling and recovery

The build runs as one small PR per issue, as 00002's did. Each issue carries its own scope and dependencies. This plan records what each one built as it lands.

## Shape

- API's library holds the run's steps, so Worker writes through the same application layer as API, per infrastructure D4. Worker's crate depends on `pipewhere-api` and keeps only Restate, the poller, and its commands.
- Worker's poller finds due work in PostgreSQL, per D5, and sends each publication's run to Restate.
- A run is a virtual object invocation whose journal is a sequence of PostgreSQL steps, per D6. D7 and D8 say what the steps do.
- `worker recover` re-drives runs in flight, per D9.
- API gains the tolerance and deadline (D11), the overdue list, and the scheduler's stall (D10).

## Steps

1. **#26 Tolerance and deadline on a publication.** `tolerance_seconds` on create and edit, `deadline` in every publication response. The migration gives existing publications 1,800 seconds. No organization setting yet: create uses 30 minutes. D1, D2, D11. Built in PR #35.
2. **#27 An organization's default tolerance.** `pipewhere.organization_setting`, `GET` and `PATCH .../settings`. Create reads the default. D11. Depends on #26.
3. **#28 The overdue list.** `?overdue=true` on the publication list. D10. Depends on #26 only for the response shape.
4. **#29 The run's steps in API's library.** The claim with its attempt, the commit claim with its deadline check, the adapter seam with no adapter, result recording, and the `cancelled` attempt result. Tested against PostgreSQL without Restate. D7, D8. Depends on #26.
5. **#30 Worker's first handler and the poller.** The `Publication` virtual object, Worker's database connection, the poller and its partial index, Compose changes, and a Restate test harness. Carries infrastructure D14's upgrade test and D15's dependency. D5, D6. Depends on #29.
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

Each organization filter, state guard, deadline condition, and table check is broken on purpose, one at a time, by a script that restores the file. Each break must fail a test.

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
  - Create takes an optional `tolerance_seconds`, and uses 1,800 when the request names none. #27 replaces that with the organization's default.
  - Edit takes an optional `tolerance_seconds`, in the same conditional update as the body and planned time, so in `scheduled` and `failed` only.
  - Retry does not touch the tolerance, so it keeps it.
  - `checked_tolerance` refuses a value outside 300 to 86,400 with 422 `invalid_body`, on create and edit.
  - Every publication response shows `tolerance_seconds` and `deadline`. The `columns!` macro computes the deadline as `planned_at + tolerance_seconds * interval '1 second'`. Nothing stores it.
  - Create's request hash gains `tolerance_seconds` when the request sends one, per D13. A request without one hashes as it did before #26, so keys stored then still match, per 00002 D5. Retry's hash is unchanged.
- The schema publishes `tolerance_seconds` with `minimum` 300 and `maximum` 86400 on `Publication`, `PublicationInput`, and `PublicationEdit`, and `deadline` as `date-time`. Both are required on `Publication`.

Choices made in the build, within the spec:

- The deadline adds a whole number of seconds. An interval of seconds holds no days, so adding it does not depend on the session's time zone.
- `tolerance_seconds: null` is treated as omitted, per D13. Create then uses 1,800, and edit keeps the current tolerance.
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

## Not yet built

#27–#33. #33 is blocked on 00004, per D12.
