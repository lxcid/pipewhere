# Plan 00003 — Scheduling and recovery

The build runs as one small PR per issue, as 00002's did. Each issue carries its own scope and dependencies. This plan records what each one built as it lands.

## Shape

- API's library holds the run's steps, so Worker writes through the same application layer as API, per infrastructure D4. Worker's crate depends on `pipewhere-api` and keeps only Restate, the poller, and its commands.
- Worker's poller finds due work in PostgreSQL, per D5, and sends each publication's run to Restate.
- A run is a virtual object invocation whose journal is a sequence of PostgreSQL steps, per D6. D7 and D8 say what the steps do.
- `worker recover` re-drives runs in flight, per D9.
- API gains the tolerance and deadline (D11), the overdue list, and the scheduler's stall (D10).

## Steps

1. **#26 Tolerance and deadline on a publication.** `tolerance_seconds` on create and edit, `deadline` in every publication response. The migration gives existing publications 1,800 seconds. No organization setting yet: create uses 30 minutes. D1, D2, D11.
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

## Not yet built

Everything above. Nothing in this plan is verified yet.
