# Plan 00010 — Agent access over MCP

The operator chose to ship 00010 as three PRs, in order. Each one is usable on its own.

1. **Review gate** (#50). An API key's publications wait in `awaiting_approval` until a member approves or cancels them in Web, per D1 and D4. An agent can already post this way through the REST API with a key.
2. **MCP server.** A stdio client package, `apps/mcp`, that calls the REST API with an organization's key. It is a client, not a fifth service, per the intent's constraint. Its create tool requires the agent's `Idempotency-Key`.
3. **Activity log.** D2 and D3, including who created each publication, which the intent asks for.

## PR 1 — Review gate

- [x] Migration widening `publication_state_check` with `awaiting_approval`.
- [x] `PublicationState::AwaitingApproval`. A create picks the state from the caller's actor.
- [x] Edit and cancel accept `awaiting_approval`.
- [x] `POST …/approve`, refusing a key with 403 `forbidden` and any other state with 409 `state_conflict`.
- [x] Web: the state's label and warning, and an Approve button beside Cancel. A waiting publication on a channel that cannot publish says so.
- [x] README: a key's publications wait for approval.

### Verified

- `cargo test -p pipewhere-api --test publication_approval`: a key's publication waits, a key cannot approve, a member approves once, and a second approval conflicts. Any member role may approve.
- `cargo test -p pipewhere-api --test publication_transitions`: the transition table now includes `awaiting_approval` and `approve`, for every state.
- `cargo test -p pipewhere-api --lib`: the schema lists the approve endpoint, the `forbidden` code, and the state.
- `cargo test -p pipewhere-worker`: the poller leaves an `awaiting_approval` publication whose time has passed untouched, with no attempt.
- `pnpm --filter @pipewhere/web test` and `typecheck`: the state's description, its approve and cancel offers, and the late-approval warning.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check`: clean.

### Failures outside this change

These were seen on the builder's machine, macOS on Darwin 27:

- `mastodon_publishing::an_instance_that_never_answers_the_connection_refuses` fails in its own setup, before any API call. The test expects the kernel to drop a connection silently once a listener's accept queue is full. This kernel resets it instead.
- `publication_runs::a_run_held_before_its_commit_continues_on_the_new_code` and `a_crash_after_the_instance_accepts_a_post_never_posts_again` failed at `send_run` in the full parallel run, which starts one Restate container per test. Each passes when run alone.

### Not verified

- Web in a browser. The Approve button and the waiting state's look were not exercised against a running stack.
