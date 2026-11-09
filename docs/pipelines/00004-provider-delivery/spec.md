# Spec 00004 — Provider delivery and Mastodon

## Decisions

### D1 — A Mastodon access token lasts until it is revoked

Proposed: awaiting operator approval.

This answers the open question "Do the Mastodon instances in scope expire access tokens?"

No. Mastodon v4.5.20's `config/initializers/doorkeeper.rb` sets `access_token_expires_in nil` and leaves `use_refresh_token` commented out. A token therefore has no expiry, and the instance issues no refresh token. On 2026-11-09, the operator's instance reported version 4.5.20 from `mastodon/mastodon` at `/api/v2/instance`.

Pipewhere stores the access token and nothing to refresh it with. A token stops working when:

- the operator revokes Pipewhere at the instance
- the account is suspended, disabled, or deleted

#43 and #44 show those in the channel's status. Reconnecting the account gives the channel a working token again.

Unknown: whether an instance on another version, or on a fork, changes this setting. An expired token answers 401, as a revoked one does, so Pipewhere would show such a channel as revoked, and reconnecting would fix it.

What would overturn this: an instance in scope whose tokens expire, often enough that reconnecting by hand is a burden.

### D2 — A channel's handle and status are shared by every provider

Proposed: awaiting operator approval.

Binding: 00008, and later pipelines that add a provider adapter or show a channel.

`pipewhere.channel` holds two fields for every provider:

- **`handle`** is the account's name at its provider, so the operator can tell channels apart. A Mastodon channel's handle is `@username@host`, where the host is the instance address the operator entered. It is null for a channel that was never connected.
- **`status`** says whether the channel can publish, and if not, why. Connecting or reconnecting sets it to `active`.

| Status | Meaning |
| --- | --- |
| `active` | Pipewhere holds access to the account and can publish to it. |
| `disconnected` | Pipewhere holds no access to the account. Channels stored before 00004 start here, because they hold no credentials. |

#43 adds a status for revoked access, and #44 one for an unavailable account. Each new value widens `ChannelStatus`, with [publication contract D8](../00002-publication-contract/spec.md)'s cost for a client that validates the enum strictly. A value arrives with the code that sets it.

The status changes only on evidence that the channel itself is broken, per [scheduling and recovery D4](../00003-scheduling-recovery/spec.md). A timeout or a server error is not such evidence.

A later pipeline is in violation if it records whether a channel can publish anywhere but this status, or names the account anywhere but this handle.

What would overturn this: a provider whose account cannot be named by one handle, or a channel that needs two statuses at once.

### D3 — An operator connects a Mastodon account through a redirect to Web

The operator chose this on 2026-11-09.

The operator enters the instance's address on the organization's page. The browser goes to the instance, the operator approves Pipewhere there, and the instance sends the browser back to Web's `/connect/mastodon` page. That page hands the code to API, which finishes the connection.

The rejected alternative was Mastodon's out-of-band code. The instance shows a code, and the operator pastes it into an API call. It needed no change to Web, but every connection took two manual API calls.

Because of this choice, Web's pages call API from the browser for the first time, per D6. [Intent 00009](../00009-web-publishing/intent.md)'s outcome to connect and reconnect channels builds on this page.

### D4 — A connection registers once per instance, and binds its redirect to the organization that started it

Proposed: awaiting operator approval.

**Registration.** API registers Pipewhere with an instance through `POST /api/v1/apps`, the first time any organization connects to it. `pipewhere.mastodon_instance` holds the client, keyed by the instance's host.

- The client is registered for one redirect address, Web's `/connect/mastodon`. If `WEB_URL` changes, the next connection registers again and replaces the row. Tokens issued to the old client keep working, because publishing needs only the token.
- If two first connections to an instance race, both register. The first client stored wins, and both send the operator to it. The other client stays registered at the instance, unused.
- The scopes are `read:accounts write:statuses write:media`. Media does not use `write:media` until [intent 00005](../00005-media-capabilities/intent.md). A scope added later would send every connected account through approval again.

**Authorization.** Starting a connection stores a row in `pipewhere.mastodon_authorization`, which records the organization and the instance. Its id travels to the instance and back as OAuth's `state`.

- Finishing checks that the authorization belongs to the request's organization, which `OrgCaller` has already authorized. Without this check, an attacker could send the operator's browser to Web's page with the attacker's own code. The attacker's account would then be linked to the operator's organization, and the operator's posts would go to it.
- The id is not a secret. A person who learns it still cannot finish a connection outside its organization.
- An authorization works once, within an hour. Finishing uses it up before calling the instance, so a code handed back twice connects once. If the instance then fails, the operator starts again. The code would be spent anyway.
- The hour covers signing in at the instance, including a second factor. Mastodon's code itself lasts 10 minutes after approval, Doorkeeper's default.
- Expired rows are deleted whenever a connection starts.

**Finishing.** API exchanges the code at `POST /oauth/token`, then reads the account through `GET /api/v1/accounts/verify_credentials`.

- **Reconnecting.** A channel is its organization's link to one account, identified by the instance and the account's id, per the intent's constraints. If the organization already links the account, the channel keeps its id, so its publications and attempts stay with it. It gets the new token and handle, and its status becomes `active`. API answers 200.
  - An account that already approved Pipewhere, and has not revoked it, sees no approval page when it reconnects. The instance sends the browser straight back with a new code. Mastodon v4.5.20's `OAuth::AuthorizationsController#render_success` skips the page whenever the account holds a token for the client with the same scopes, unless the request sets `force_login`. The operator saw this on their instance on 2026-11-10.
- **A new channel.** If the organization does not link the account, API creates a channel and answers 201.
- **Organizations stay apart.** Several organizations can link one account. The lookup reads only the request's organization's channels, per [publication contract D7](../00002-publication-contract/spec.md).

**Web's part.** Every organization registers the same redirect address, so the redirect cannot say which organization started it. Web keeps the organization's id in `sessionStorage` under the authorization's id while the browser is at the instance. A redirect that reaches another tab finds nothing there, and asks the operator to start again from the organization's page.

**Error codes.**

| Code | Status | Meaning |
| --- | --- | --- |
| `authorization_not_found` | 422 | No authorization in the organization has this id, or it was used, or it is over an hour old. |
| `authorization_refused` | 422 | The instance refused the code, or the account. |
| `instance_unavailable` | 502 | The instance could not be reached, or did not answer as Mastodon does. API logs what it answered. |

Each code widens `ErrorCode`, with publication contract D8's cost.

**An instance's address** is a host, with a port when it is not 443, optionally after `https://`. API refuses an address with a path, a user, a query, or another scheme, with 422 `invalid_body`. An operator who pastes their handle, such as `@name@mastodon.social`, is refused rather than guessed at.

Tolerated:

- **Two finishes of one account at the same moment can create two channels.**
  - Likelihood: it needs two approvals of the same account, in one organization, finishing within one transaction of each other.
  - Impact: the organization has two channels for one account.
  - Recoverability: nothing in the API removes a channel yet.
  - Cost: a unique constraint needs the organization on `mastodon_channel`. That copy could disagree with the channel's, unless a composite foreign key keeps them equal, as [publication contract D19](../00002-publication-contract/spec.md) found for attempts.
- **API sends requests to any host a member names, including a private address.** A self-hosting operator can only reach their own network this way. A hosted deployment must refuse private and loopback addresses before strangers can create organizations.

### D5 — Provider secrets are sealed with AES-256-GCM under `CREDENTIALS_KEY`

Proposed: awaiting operator approval.

Binding: 00008, and later pipelines that store a provider secret.

The intent requires that "Provider secrets must not be stored in plain text." This decision says how.

- **The key** is 32 bytes, written in `.env` as 64 hex characters. It has no default, per [infrastructure D7](../00001-infrastructure/spec.md). API refuses to start without it.
- **Each value** is sealed with a fresh random 96-bit nonce. The column holds the nonce, then the ciphertext and its tag.
- **The row's owner is the associated data.** That is the channel's id for a token, and the instance's host for a client secret. A value copied to another row does not open.
- **The library** is ring 0.17.14, which rustls already compiles. No other cryptography crate is added.
- **Random nonces** are safe for about 2^32 seals under one key. A secret is sealed once per connection.

The key is not rotated. A changed key makes every stored secret unreadable. API then answers 500, and logs which secret did not open and that the key it was sealed under must be restored. The README asks operators to back up the key with the database. Rotation waits for a deployment that needs it.

A later pipeline is in violation if it stores a provider secret in another form.

What would overturn this: a need to rotate keys, or a hosted deployment that keeps its keys in a key management service.

### D6 — API allows Web's origin

Proposed: awaiting operator approval.

D3 makes Web's pages call API from the browser. API answers CORS requests from `WEB_URL`'s origin, for `GET`, `POST`, and `PATCH`, with the headers `authorization`, `content-type`, and `idempotency-key`.

- **No credentials.** A page gets a token from Auth and sends it in the `authorization` header, as every client does, per [infrastructure D2](../00001-infrastructure/spec.md). API reads no cookie, so Web and API need not share a site, unlike Web and Auth under infrastructure D8.
- **Web learns API's address** from `API_URL`, read per request like `AUTH_URL`. One Web image therefore serves any deployment, per infrastructure D3.
- **Another origin.** API names Web's origin to every origin that asks, and the browser refuses the request from any other.
