---
status: draft
---

# Intent 00003 — Scheduling and recovery

## Problem

A queued publication's time depends on posting slots, earlier queued work, and the organization's timezone. These can change after it joins the queue. Daylight-saving changes create missing or repeated local times. If the worker is down or a channel cannot publish, a planned time can pass. Losing Restate's execution state must not lose the schedule stored in PostgreSQL or publish twice during recovery.

## Proposed outcome

An operator can publish now, pin a publication to a time, or add it to a channel's next posting slot. The API shows the current schedule and whether a time is fixed or projected. An operator can change or cancel a publication before its provider commit starts. Late work follows an explicit deadline policy. A recovery command restores scheduling from PostgreSQL without repeating an ambiguous commit, per [publication contract D4](../00002-domain-model/spec.md).

## Affected users and systems

Operators and API clients scheduling work; the API, PostgreSQL, and Restate worker that claim and run it.

## Constraints

- The [publication contract](../00002-domain-model/intent.md) supplies the states and retry boundary.
- PostgreSQL owns schedules; Restate holds execution state only, per [infrastructure D4](../00001-infrastructure/spec.md).
- A fixed time must not silently move when posting slots change.

## Open questions

- Does a queued publication keep a fixed time when it enters the queue, or move when slots and earlier work change?
- How are missing and repeated local slot times resolved?
- How late may a provider commit start after its planned time?
- What happens to an unclaimed queue when its channel cannot publish?
