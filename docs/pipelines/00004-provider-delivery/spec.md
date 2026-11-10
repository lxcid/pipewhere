# Spec 00004 — Provider delivery and Mastodon

## Decisions

### D1 — A Mastodon access token lasts until it is revoked

This answers the open question "Do the Mastodon instances in scope expire access tokens?"

No. Mastodon v4.5.20's `config/initializers/doorkeeper.rb` sets `access_token_expires_in nil` and leaves `use_refresh_token` commented out. A token therefore has no expiry, and the instance issues no refresh token. On 2026-11-09, the operator's instance reported version 4.5.20 from `mastodon/mastodon` at `/api/v2/instance`.

Pipewhere stores the access token and nothing to refresh it with. A token stops working when:

- the operator revokes Pipewhere at the instance
- the account is suspended, disabled, or deleted

#43 and #44 show those in the channel's status. Reconnecting the account gives the channel a working token again.

Unknown: whether an instance on another version, or on a fork, changes this setting. An expired token answers 401, as a revoked one does, so Pipewhere would show such a channel as revoked, and reconnecting would fix it.

What would overturn this: an instance in scope whose tokens expire, often enough that reconnecting by hand is a burden.

### D2 — A channel's handle and status are shared by every provider

Binding: 00008, and later pipelines that add a provider adapter or show a channel.

`pipewhere.channel` holds two fields for every provider:

- **`handle`** is the account's name at its provider, so the operator can tell channels apart. A Mastodon channel's handle is `@username@host`, where the host is the instance address the operator entered. It is null for a channel that was never connected.
- **`status`** says whether the channel can publish, and if not, why. Connecting or reconnecting sets it to `active`.

| Status | Meaning |
| --- | --- |
| `active` | Pipewhere holds access to the account and can publish to it. |
| `disconnected` | Pipewhere holds no access to the account. Channels stored before 00004 start here, because they hold no credentials. |
| `access_revoked` | The provider no longer accepts Pipewhere's access to the account. Connecting it again fixes this. Set per D9. |

A channel stored before 00004 cannot be reconnected. No account is recorded for it, so connecting its account creates a new channel, and the old one keeps its publications. This is tolerated: before 00004 no endpoint created channels, so such rows exist only where someone inserted them by hand.

#44 adds a status for an unavailable account. Each new value widens `ChannelStatus`, with [publication contract D8](../00002-publication-contract/spec.md)'s cost for a client that validates the enum strictly. A value arrives with the code that sets it.

The status changes only on evidence that the channel itself is broken, per [scheduling and recovery D4](../00003-scheduling-recovery/spec.md). A timeout or a server error is not such evidence.

A later pipeline is in violation if it records whether a channel can publish anywhere but this status, or names the account anywhere but this handle.

What would overturn this: a provider whose account cannot be named by one handle, or a channel that needs two statuses at once.

### D3 — An operator connects a Mastodon account through a redirect to Web

The operator chose this on 2026-11-09.

The operator enters the instance's address on the organization's page. The browser goes to the instance, the operator approves Pipewhere there, and the instance sends the browser back to Web's `/connect/mastodon` page. That page hands the code to API, which finishes the connection.

The rejected alternative was Mastodon's out-of-band code. The instance shows a code, and the operator pastes it into an API call. It needed no change to Web, but every connection took two manual API calls.

Because of this choice, Web's pages call API from the browser for the first time, per D6. [Intent 00009](../00009-web-publishing/intent.md)'s outcome to connect and reconnect channels builds on this page.

### D4 — A connection registers once per instance, and binds its redirect to the organization that started it

**Registration.** API registers Pipewhere with an instance through `POST /api/v1/apps`, the first time any organization connects to it. `pipewhere.mastodon_instance` holds the client, keyed by the instance's host.

- The client is registered for one redirect address, Web's `/connect/mastodon`. If `WEB_URL` changes, the next connection registers again and replaces the row. Tokens issued to the old client keep working, because publishing needs only the token. A connection also registers again when the stored client secret no longer opens, per D5.
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
- **An instance that forgets Pipewhere's client refuses every connection to it.** An admin may delete the app, or restore a database from before the registration. Mastodon v4.5.20 deletes no app by itself: its scheduled vacuums remove expired and revoked tokens and grants, but no application.
  - Likelihood: it needs an unusual act by the instance's admin.
  - Impact: no organization can connect or reconnect through that instance. The instance shows its own error instead of the approval page, or the token step answers `authorization_refused`.
  - Recoverability: only by SQL. Setting the instance row's `redirect_uri` to any other value makes the next connection register again. Deleting the row is blocked by the channels and authorizations that refer to it.
  - Cost: registering again on a 401 `invalid_client` would be small, but it adds behavior for a case no operator has met.
- **API sends requests to any host a member names, including a private address.** A self-hosting operator can only reach their own network this way. A hosted deployment must refuse private and loopback addresses before strangers can create organizations.

### D5 — Provider secrets are sealed with AES-256-GCM under `CREDENTIALS_KEY`

Binding: 00008, and later pipelines that store a provider secret.

The intent requires that "Provider secrets must not be stored in plain text." This decision says how.

- **The key** is 32 bytes, written in `.env` as 64 hex characters. It has no default, per [infrastructure D7](../00001-infrastructure/spec.md). API refuses to start without it.
- **Each value** is sealed with a fresh random 96-bit nonce. The column holds the nonce, then the ciphertext and its tag.
- **The row's owner is the associated data.** That is the channel's id for a token, and the instance's host for a client secret. A value copied to another row does not open.
- **The library** is ring 0.17.14, which rustls already compiles. No other cryptography crate is added.
- **Random nonces** are safe for about 2^32 seals under one key. A secret is sealed once per connection.

The key is not rotated. A changed key makes every stored secret unreadable. The README asks operators to back up the key with the database. Rotation waits for a deployment that needs it. An operator who loses the key recovers by connecting every channel again:

- Starting a connection registers Pipewhere with the instance again when the stored client secret does not open. Only the first of two such connections replaces it, so both send the operator to the same client.
- Finishing it seals the channel's new token under the current key.

Until then, API answers 500 wherever it needs a secret that does not open. It logs which secret it was, and that the old key must be restored or the account connected again.

A later pipeline is in violation if it stores a provider secret in another form.

What would overturn this: a need to rotate keys, or a hosted deployment that keeps its keys in a key management service.

### D6 — API allows Web's origin

D3 makes Web's pages call API from the browser. API answers CORS requests from `WEB_URL`'s origin, for `GET`, `POST`, and `PATCH`, with the headers `authorization`, `content-type`, and `idempotency-key`.

- **No credentials.** A page gets a token from Auth and sends it in the `authorization` header, as every client does, per [infrastructure D2](../00001-infrastructure/spec.md). API reads no cookie, so Web and API need not share a site, unlike Web and Auth under infrastructure D8.
- **Web learns API's address** from `API_URL`, read per request like `AUTH_URL`. One Web image therefore serves any deployment, per infrastructure D3.
- **Another origin.** API names Web's origin to every origin that asks, and the browser refuses the request from any other.

### D7 — A Mastodon post's `Idempotency-Key` is its publication's id

This answers the open question "Within its hour, does a repeated `Idempotency-Key` return the original status, so a lost response can be settled without a second commit?"

Yes, once the first request has saved its status. Mastodon v4.5.20's `PostStatusService` handles the key this way:

- It stores the key in Redis as `idempotency:status:{account_id}:{key}` for 3600 seconds, after the status is saved.
- Before posting, it looks the key up. If the key is stored, it answers the status the key names and posts nothing. The request's text is ignored.
- If that status was deleted, `statuses.find` raises `ActiveRecord::RecordNotFound`, and the instance answers 404.
- It takes no lock. Two requests with one key that overlap both post, because neither finds the key stored.

So committing again with the same key within the hour settles a lost response, and posts nothing new. Pipewhere still never commits again by itself after an ambiguous result, per [publication contract D4](../00002-publication-contract/spec.md). The key protects a person's retry: if the first post exists, the retry answers it, and the publication records it as published.

The key is the publication's id, not the attempt's. A retry is a new attempt, so a key per attempt would post again.

This narrows [scheduling and recovery D19](../00003-scheduling-recovery/spec.md) and does not overturn it. An old call and a retry that overlap both post, so the time limit in D8 still matters.

A 404 has its own reason, so the operator knows to wait out the hour rather than look for a fault.

Tolerated:

- **An edited publication retried within the hour answers the post it made before the edit.** The publication records that post as published, with its old text.
  - Likelihood: it needs an ambiguous result whose post exists, and a person who resolves it as failed rather than published, then edits and retries within the hour. The reason tells them to check the account first.
  - Impact: the record names a post with the old text.
  - Recoverability: the link shows the old text. The operator deletes the post at the instance and creates a new publication.
  - Cost: a key that adds a hash of the body would post the edited text instead. If the old post still exists, that is a second public post, which publication contract D4 counts as the failure that cannot be undone.
- **A retry within the hour of a deleted post fails.** The reason says to retry an hour after the post was made.

What would overturn this: an instance version whose repeated key does something else, or a need to repost an edited publication within the hour.

### D8 — How the Mastodon adapter maps an instance's answer

The adapter posts a publication's body to `POST /api/v1/statuses` with the channel's token and D7's key. It maps the answer to a result for [publication contract D3 and D4](../00002-publication-contract/spec.md):

| Answer | Result | Why |
| --- | --- | --- |
| 2xx with a status id | `Published` | |
| 2xx with a blank or unreadable id, or a body that is not a status | `Ambiguous` | A post may exist, and the answer cannot name it. |
| 401 | `Refused`, and the channel becomes `access_revoked` | D9. |
| 404 | `Refused`, saying when to retry | D7. |
| 3xx | `Refused`, without following it | See below. |
| Any other 4xx | `Refused`, with the instance's error | The instance refused the post before saving it. |
| 5xx | `Ambiguous` | A server error does not show that nothing was posted. |
| No answer within 30 seconds | `Ambiguous` | [Scheduling and recovery D19](../00003-scheduling-recovery/spec.md). |
| No connection made | `Refused` | The request was never sent. |
| A connection lost after it was made | `Ambiguous` | The request may have arrived. |

Each `Ambiguous` reason tells the operator to check the account before resolving the publication.

**Why a 4xx means nothing was posted**, in Mastodon v4.5.20:

- 401 and 403 come from `Api::BaseController`'s checks, which run before the action.
- 422 comes from a validation, which rolls the status back.
- 429 is raised by `RateLimitable` in `after_create`, inside the status's transaction, so the status is rolled back too. The limit is 300 statuses per account in 3 hours.
- Nothing in the statuses controller redirects. A 3xx comes from the instance's front, before Mastodon sees the request.

**Redirects.** Mastodon calls do not follow redirects. reqwest 0.13.5 follows up to 10 by default. Following one would send a post again, for a 307 or 308. Following one to another host also drops the token. That host's 401 would then revoke a working channel. A test shows that happening under reqwest's default. This applies to connecting too, where a redirect now answers `instance_unavailable` and API logs it.

**The time limit** is 30 seconds. It is short compared with a person checking the account, as D19 asks. Restate 1.7.13 asks an invocation that has been silent for a minute to suspend, according to its `inactivity-timeout`. Ending the call well within that keeps Restate out of a commit in flight. A post normally takes well under a second. Making the connection has a shorter limit of its own, per D10.

**[Scheduling and recovery D18](../00003-scheduling-recovery/spec.md).** A blank id, or one holding U+0000, answers `Ambiguous`, because the post may exist. A `url` that is not http or https is dropped, and the post keeps its id. Refusing the answer would lose the record of a post that exists. API logs the dropped address.

**An instance's error** is cut to 300 characters, with U+0000 removed, before it becomes a reason. PostgreSQL text cannot hold U+0000. A reason that cannot be stored leaves the post's outcome unrecorded.

**Before the call**, the adapter refuses when:

- Pipewhere holds no token for the channel.
- The token does not open under `CREDENTIALS_KEY`, per D5. The reason says to restore the key or connect the account again.
- The channel's access cannot be read.

A channel of another provider answers as before: Pipewhere cannot publish to it yet.

Tolerated:

- **A brief failure fails the publication, rather than running again before its deadline.** This covers an instance that cannot be reached at the planned time, and a 429. [Scheduling and recovery D4](../00003-scheduling-recovery/spec.md) allows a result known to have done nothing to run again, but does not require it.
  - Likelihood: the instance must be down at the moment of the commit. The rate limit is far above one operator's posting.
  - Impact: one failed publication, with the reason.
  - Recoverability: the operator retries it.
  - Cost: `CommitResult` would need a result that asks for a later run, and a run would need to schedule one.
- **A 403 fails the publication, but does not change the channel's status.** This covers a disabled, suspended, or unapproved login. #44 adds the status for an unavailable account.

What would overturn this: a Mastodon version or fork that saves a status and then answers 4xx, or brief failures common enough that operators retry by hand often.

### D9 — A token the instance no longer accepts revokes the channel's access

When an instance answers 401 to a post, the adapter sets the channel's status to `access_revoked`, per D2. The publication fails with a reason that says to connect the account again, then retry. Reconnecting sets the status back to `active`, per D4. Web shows the status on the Channels card.

A 401 is evidence that the channel itself is broken, as [scheduling and recovery D4](../00003-scheduling-recovery/spec.md) requires:

- `Api::BaseController` answers 401 only for a token Doorkeeper does not accept: unknown, revoked, or expired.
- No redirect takes the request to another host, per D8, so the 401 is the instance's own.

Once 00003's #33 lands, the worker reads the status when it claims a commit, and fails later publications to the channel before calling the instance. Until then, each one fails on its own 401.

If the status cannot be written, API logs it. The publication's reason still says to connect again.

Tolerated:

- **A post's 401 that arrives after a reconnect revokes the new access.** The update does not check which token the 401 answered.
  - Likelihood: the post must use the old token, and its answer must arrive after the reconnect commits. That window is one request long.
  - Impact: a working channel shows `access_revoked`.
  - Recoverability: the operator connects the account again.
  - Cost: the update would compare the token it used, or a version, with the channel's current one.

What would overturn this: an instance that answers 401 for a reason other than the token, such as a proxy's own authentication.

### D10 — Connecting to an instance has a 5-second limit

Every call to a Mastodon instance has 5 seconds to make its connection, which covers DNS, TCP, and TLS. D8's 30-second limit still covers the whole call.

This is what makes D8's row "No connection made" refused. reqwest 0.13.5 reports a connection error only when the connection's own limit ends the wait. Without one, a host that drops packets let the 30-second limit end the wait instead. reqwest reports that as a timeout, so the post became `Ambiguous`, though no request was sent.

- Connecting to an instance normally takes well under a second. 5 seconds allows for a lost packet or two.
- A connection slower than that fails the publication as unreachable. The reason gives the cause, and the operator retries it.
- Connecting an account gets the same limit, inside its own 10 seconds.
- The tests' plain-HTTP client uses 1 second, because a server on the same machine accepts a connection at once or not at all.

What would overturn this: an instance in scope that often takes more than 5 seconds to connect.
