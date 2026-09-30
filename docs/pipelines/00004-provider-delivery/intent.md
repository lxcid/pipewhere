---
status: draft
---

# Intent 00004 — Provider delivery

## Problem

Mastodon and Threads create posts differently. Mastodon uses one publish call; Threads creates a container and later publishes it, and caps daily publishes. A lost response can leave either outcome unknown. Credentials can expire or be revoked, and a provider account may be removed. The operator needs to know which channel cannot publish and why a publication failed.

## Proposed outcome

Mastodon and Threads adapters publish through the [publication contract](../00002-domain-model/intent.md). Each commit has an attempt record, a clear failure reason, and a tested response to ambiguous outcomes. A channel has a stable identity and a readable status. Reconnecting it preserves its history. Credential storage, refresh, and manual recovery keep working across restarts.

## Affected users and systems

Operators connecting channels and resolving failures; API, provider adapters, PostgreSQL, Restate, and the two providers.

## Constraints

- Never automatically repeat an ambiguous commit, per [publication contract D4](../00002-domain-model/spec.md).
- A Mastodon account is identified by its instance and account id. A channel cannot be held by two organizations at once.
- Provider secrets must not be stored in plain text.

## Open questions

- If `threads_publish` is re-issued with an already-published `creation_id`, does it deduplicate, and can container status settle a timed-out publish?
- Does owned-account access to Threads require full app review, or can development-mode tester access suffice?
- Do the Mastodon instances in scope expire access tokens?
