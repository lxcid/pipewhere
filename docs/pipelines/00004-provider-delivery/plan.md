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
- **Credentials.** `src/credentials.rs` seals tokens and client secrets under `CREDENTIALS_KEY`, per D5. API refuses to start without the key, or with one that is not 64 hex characters. When a stored client secret no longer opens, starting a connection registers Pipewhere with the instance again, so an operator who loses the key can connect every channel again.
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
- On 2026-11-10, the operator connected their own account on their Mastodon 4.5.20 instance. They used a second throwaway Compose stack on other ports, built from the same code. In Chrome, they started from the Channels card, approved Pipewhere at the instance, and came back through `http://localhost:13000/connect/mastodon`. Then:
  - The organization's page listed the channel with its handle, as able to publish.
  - The database held one `active` channel with the account's id. Its token was sealed to 71 bytes: the nonce, the token, and the tag.
  - The instance's row held the client registered for Web's redirect address. No authorization was left.
  - API logged no warning.
- At `dfa60be`, which fixes recovery from a lost key: `cargo test -p pipewhere-api` passes 108 tests. The new `reconnecting_after_the_key_is_lost_registers_again` seals the stored client secret and token under another key, as a lost key leaves them. Reconnecting then registers again, keeps the channel's id, and stores a token the running key opens. Before the fix, it answered 500. Clippy, fmt, and the Web typecheck are clean.

### Not verified

- The connect page's error messages in a browser: a refused approval, an expired authorization, and a redirect that reaches another tab. Only a successful connection was seen.
- The migration over existing channel rows. It ran only on new databases. Existing rows take the column's default, `disconnected`.

## #43 — what was built

Built in PR #46. D7 to D10.

- **Adapter.** `apps/api/src/mastodon/publish.rs` posts a publication's body to `POST /api/v1/statuses`, with the publication's id as its `Idempotency-Key`, per D7. It maps each answer to a result per D8, and ends the call after 30 seconds. A channel of another provider answers as `NoAdapter` did.
- **Connection limit.** Connecting to an instance has its own limit of 5 seconds, per D10. reqwest reports a connection error only when that limit ends the wait. Without it, a host that drops packets let the 30-second limit end the wait, and a post that was never sent became `outcome_unknown`. With it, the post is refused, as D8's table says.
- **Reasons.** A failed request's reason carries its causes, such as `tcp connect error: Connection refused`. reqwest's own text names only the request.
- **Revoked access.** A 401 sets the channel's status to `access_revoked`, per D9. A migration widens the status check, `ChannelStatus` gains the value, and Web's Channels card says to connect the account again.
- **Redirects.** Mastodon calls no longer follow redirects, per D8. This also covers connecting, where a redirect answers `instance_unavailable`.
- **Worker** reads `CREDENTIALS_KEY` and runs the adapter. Compose passes Worker the key.
- **The commit step** hands the adapter the publication's and the channel's ids.

### Verified

At `6ad86de`:

- `cargo test -p pipewhere-api`: 123 tests pass. 15 are new in `tests/mastodon_publishing.rs`. They call the adapter directly, against the fake instance, which now answers `POST /api/v1/statuses` as Mastodon v4.5.20's `PostStatusService` does. They cover:
  - a post carrying the body and the key, recorded with its id and address
  - a repeated key, which answers the first status and posts nothing
  - a revoked token, which refuses and marks the channel `access_revoked`
  - 403, 422, 429, and 400, which refuse with the instance's error and leave the channel `active`
  - a 404, whose reason says when to retry
  - a 307 to another host, which refuses without following it. Under reqwest's default policy this test fails: the token is dropped, the 401 revokes the channel.
  - 500, 502, an HTML 200, and a 200 with a blank id, which are ambiguous
  - an address that is not http or https, which is dropped while the id is kept
  - an instance that answers too late, with a 300 ms limit, which is ambiguous
  - an unreachable instance, a channel with no token, a token sealed under another key, and a Threads channel, which refuse. The unreachable instance's reason says the connection was refused.
  - a host that never answers the connection, which refuses after the connection limit. The test fills a local listener's accept queue, so the kernel drops each new connection, and gives the request a 5-second limit. That is shorter than the 8 seconds macOS waits before it ends a loopback connection itself. Without the connection limit, this test answers `Ambiguous` after 5 seconds.
  - an error over 300 characters holding U+0000, which is cut and cleaned
- `cargo test -p pipewhere-worker`: 11 pass. 2 are new in `tests/publication_runs.rs`, through Restate 1.7.13:
  - A due publication posts once, and records the status's id, address, and the published attempt.
  - The instance saves the post and holds its answer. Worker is dropped, as a crash drops it, and a new Worker starts on the same port. The publication becomes `outcome_unknown`, with one attempt, and the instance received one request. This is the crash after acceptance that publication contract D4 asks the adapter's pipeline to test.
- `pnpm --filter @pipewhere/auth test`: 20 pass.
- `pnpm --filter @pipewhere/web typecheck`, clippy, and fmt are clean.
- The Mastodon facts that D7 to D9 rest on were read in Mastodon v4.5.20's source: `PostStatusService`, `Api::V1::StatusesController`, `Api::ErrorHandling`, `RateLimitable`, and `RateLimiter`. reqwest 0.13.5's redirect handling was read in its source. Restate 1.7.13's timeouts come from `--dump-config` on its image.
- On 2026-11-10, with the operator's go-ahead, the throwaway Compose stack from #42 was rebuilt at `2b558c1`. `api-migrate` applied the migration over the channel connected in #42, which stayed `active`. Then a publish-now publication went to the operator's account on their Mastodon 4.5.20 instance, through the API:
  - The instance answered 401. Its `WWW-Authenticate` said `The access token was revoked`, and a read-only `verify_credentials` with the stored token answered the same. The token had been revoked at the instance after #42's connection. The publication failed in 130 ms with D9's reason, and the channel became `access_revoked`.
  - Before reconnecting, the operator saw the Channels card say `Access revoked. Connect the account again.`
  - The operator connected the account again. The channel kept its id and became `active`.
  - A retry of the same publication was `published` in 156 ms, with the status's id and address. The instance's public API showed that status once, with the publication's text, as the account's newest. The publication holds two attempts: the refusal and the published one.

### Not verified

- A real instance's answers other than 401 and a successful post. In particular, the 404 for a deleted post follows the source, through the fake.
