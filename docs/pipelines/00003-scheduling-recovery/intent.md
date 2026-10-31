---
status: draft
---

# Intent 00003 — Scheduling and recovery

## Problem

A publication waits for its planned time and then publishes. While it waits, the worker can be down or its channel unable to publish, so the time passes. A worker that stops picking up work may say nothing, and the operator learns only when a post is missing. Re-running everything that still looks pending can publish twice if a commit had already started. Losing Restate's execution state must not lose the schedule stored in PostgreSQL.

## Proposed outcome

An operator can publish now or pin a publication to a time. They can change it while it waits and cancel it before its provider commit starts. Late work follows an explicit deadline policy. A recovery command restores scheduling from PostgreSQL without repeating an ambiguous commit, per [publication contract D4](../00002-publication-contract/spec.md).

The outcome is true when:

- The API shows each waiting publication's planned time.
- An operator can see through the API which publications passed their planned time without starting, without reading worker logs.
- When scheduled work stops running, the operator can tell from Pipewhere, not only from a missing post.
- While a publication waits, the API shows when its channel cannot currently publish, and why.

## Affected users and systems

Operators and API clients scheduling work; the API, PostgreSQL, and Restate worker that claim and run it.

## Constraints

- The [publication contract](../00002-publication-contract/intent.md) supplies the states and retry boundary.
- PostgreSQL owns schedules; Restate holds execution state only, per [infrastructure D4](../00001-infrastructure/spec.md).
- A pinned time never moves unless the operator changes it.
- A channel's readable status comes from [intent 00004](../00004-provider-delivery/intent.md).
- Posting slots are out of scope; [intent 00007](../00007-posting-slots/intent.md) owns them.

## Open questions

None. The questions about posting slots moved to intent 00007. The two that remained are answered in the [spec](spec.md).
