---
status: draft
---

# Intent 00004 — Provider delivery and Mastodon

## Problem

Pipewhere needs a channel model and a delivery path that every provider adapter uses. Mastodon is the first provider. It publishes with one call, but each account lives on one of many instances.

A self-hosting operator should not have to register an app with a provider before connecting a channel. Mastodon makes that possible: [`POST /api/v1/apps`](https://docs.joinmastodon.org/methods/apps/) is a public endpoint, so Pipewhere can register itself with whichever instance the operator names.

A lost response can leave a publish outcome unknown. Mastodon deduplicates [`POST /api/v1/statuses`](https://docs.joinmastodon.org/methods/statuses/) by an `Idempotency-Key` header, but stores keys for up to 1 hour. Its documentation does not say what a repeated key returns.

Credentials can be revoked, an instance can be unreachable, and an account may be removed. When a channel cannot connect or publish, the operator needs Pipewhere to say which channel and why. A generic error does not tell them what to fix.

## Proposed outcome

- An operator connects a Mastodon account on any instance by entering the instance's address. There is no app registration step for the operator.
- The Mastodon adapter publishes through the [publication contract](../00002-publication-contract/intent.md). Each commit has an attempt record, a clear failure reason, and a tested response to ambiguous outcomes.
- A channel has a stable identity and a readable status. When it cannot connect or publish, its status says why.
- Reconnecting a channel preserves its history.
- Credential storage and reconnection keep working across restarts.

## Affected users and systems

Operators connecting channels and resolving failures; API, the Mastodon adapter, PostgreSQL, Restate, and Mastodon instances.

## Constraints

- Never automatically repeat an ambiguous commit, per [publication contract D4](../00002-publication-contract/spec.md).
- A Mastodon account is identified by its instance and account id. A channel links one external account to one organization. Several organizations may link the same account, per [publication contract D7](../00002-publication-contract/spec.md). Their channels are separate, and none learns of another's.
- Channel and attempt records share one provider-neutral shape, so [Threads delivery](../00008-threads-delivery/intent.md) adds an adapter rather than reshaping them. Provider data that must survive a restart, such as an instance's registered client, is still stored in PostgreSQL, per [infrastructure D4](../00001-infrastructure/spec.md). It belongs to the adapter, not to the shared records.
- Provider secrets must not be stored in plain text.
- Threads delivery is out of scope; intent 00008 owns it.

## Open questions

- Do the Mastodon instances in scope expire access tokens? Mastodon's [OAuth documentation](https://docs.joinmastodon.org/methods/oauth/) describes neither token expiry nor refresh tokens.
- Within its hour, does a repeated `Idempotency-Key` return the original status, so a lost response can be settled without a second commit?
