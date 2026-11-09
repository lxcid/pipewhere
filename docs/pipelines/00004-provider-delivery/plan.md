# Plan 00004 — Provider delivery and Mastodon

The build runs as one PR per issue, as 00002's and 00003's did. Each issue carries its own scope and dependencies. Each PR records the decisions it needs in the [spec](spec.md). This plan records what each one built as it lands.

## Shape

- A channel keeps one provider-neutral record. `pipewhere.channel` gains a handle and a status, which Threads will reuse.
- Mastodon's own data lives in tables that belong to its adapter: each instance's registered client, and each channel's instance, account id, and token.
- API connects an account. Web hosts the page Mastodon redirects to and hands the code to API. Web holds no secret, per infrastructure D2.
- Worker's Mastodon adapter replaces `NoAdapter` for Mastodon channels.
- Worker checks each channel's access on an interval, so a revoked channel shows before its publications fail.

## Steps

1. **#42 Connect and reconnect a Mastodon account.** Instance registration, the authorization redirect through Web, the token exchange, encrypted credentials, and the channel's handle and status. API allows Web's origin. Answers the intent's question on token expiry.
2. **#43 Publish to Mastodon.** The adapter, its `Idempotency-Key`, its time limit, how each answer maps to a result, and a revoked token setting the channel's status. Answers the intent's question on `Idempotency-Key`. Depends on #42.
3. **#44 Check each Mastodon channel's access.** Depends on #42. It changes Worker's poller, so it follows #40.
4. **00003's #33** then reads the channel's status at the claim and shows it on a waiting publication.

## Verification approach

Each issue runs, at its head:

- `cargo test -p pipewhere-api` and `cargo test -p pipewhere-worker`
- `pnpm --filter @pipewhere/auth test`
- `pnpm --filter @pipewhere/web typecheck`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all --check`

The tests need `pipewhere-test-postgres`, started by `pnpm --filter @pipewhere/auth test:db`, and Worker's tests need a Restate server.

Mastodon is faked by a local HTTP server in the tests. It answers as Mastodon v4.5.20's source says the real endpoints do, and the tests cite the source they follow.

The last step connects the operator's own account on a live instance and publishes to it. It needs the operator's go-ahead, because it posts.
