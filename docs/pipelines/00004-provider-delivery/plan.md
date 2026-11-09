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

## #42 — what was built

Built in PR #45. D1 to D6.

- **Migration.** `pipewhere.channel` gains `handle` and `status`. Existing channels become `disconnected`, per D2. Three tables belong to Mastodon's adapter: `mastodon_instance`, `mastodon_authorization`, and `mastodon_channel`, per D4.
- **API.** `POST .../channels/mastodon/authorizations` registers Pipewhere with the instance when needed, and answers where to send the operator. `POST .../channels/mastodon` finishes the connection, answering 201 for a new channel and 200 for a reconnected one. Channel responses show `handle` and `status`.
- **Credentials.** `src/credentials.rs` seals tokens and client secrets under `CREDENTIALS_KEY`, per D5. API refuses to start without the key, or with one that is not 64 hex characters.
- **CORS.** API answers Web's origin, per D6.
- **Web.** A Channels card on the organization's page lists the channels and starts a connection. `/connect/mastodon` receives the instance's redirect and hands the code to API.
- **Configuration.** API takes `WEB_URL` and `CREDENTIALS_KEY`, and Web takes `API_URL`. The README generates the key.

Calls to the instance while connecting time out after 10 seconds, because an operator waits on each of them.

### Verified

At `ff5dc6a`:

- `cargo test -p pipewhere-api`: 107 tests pass. 10 are new in `tests/mastodon_channels.rs`, against a fake instance in `tests/support/mastodon.rs` that answers as Mastodon v4.5.20's apps controller, Doorkeeper's token endpoint, and `verify_credentials` do. They cover:
  - connecting
  - reconnecting the same account, which keeps the channel's id and its publication
  - two organizations linking one account
  - registering once per instance, and again when Web's address changes
  - an authorization used twice, used from another organization, and over an hour old
  - a refused code
  - an unreachable instance, and a server that is not Mastodon
  - addresses that are not an instance's
  - an outsider
  - Web's CORS preflight
- The unit tests cover the credentials, including a key read again, as after a restart, opening what was sealed. They also cover the address rules and the redirect address.
- `cargo test -p pipewhere-worker`: 9 pass. `pnpm --filter @pipewhere/auth test`: 20 pass.
- `pnpm --filter @pipewhere/web typecheck`, clippy, and fmt are clean.
- A real Compose stack, in a throwaway project on other ports, built from the README's configuration with `CREDENTIALS_KEY` and `API_URL` added. `up -d --build` exits 0, and `api-migrate` applies the migration to a new database. Then, with `curl`:
  - A new organization lists no channels.
  - A handle as the address answers 422 `invalid_body`. `localhost:1` answers 502 `instance_unavailable`, and API logs why.
  - `example.com` answers 502, and API logs `https://example.com/api/v1/apps answered 405 Method Not Allowed`. This shows API's image makes HTTPS calls, which no image had done before.
  - An unknown authorization answers 422 `authorization_not_found`.
  - A preflight from Web's origin answers 200, naming that origin, the three methods, and the three headers.
  - Web serves `/`, the organization's page, and `/connect/mastodon` with 200. `/openapi.json` lists both new paths.
  - `CREDENTIALS_KEY=not-hex` stops API at start, with the message saying how to generate the key.

### Not verified

- Connecting a real account on a real instance. It needs the operator to sign in and approve at the instance.
- The Web pages in a browser. Only their HTTP status was checked, and the TypeScript compiles.
- The migration over existing channel rows. It ran only on new databases. Existing rows take the column's default, `disconnected`.
