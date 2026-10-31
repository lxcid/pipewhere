---
status: draft
---

# Intent 00007 — Posting slots

## Problem

An operator often wants a publication to go out at a channel's next posting slot rather than at a time they pick. Slots are local times in a timezone. A queued publication's time therefore depends on the channel's slots, on earlier queued work, and on the timezone that resolves those slots. Any of these can change after it joins the queue. Daylight-saving changes create missing or repeated local times.

## Proposed outcome

An operator can set a channel's posting slots and add a publication to the channel's next free slot. The API shows each publication's time and whether that time is fixed or projected. Changing slots never moves a time the operator fixed.

## Affected users and systems

Operators and API clients queueing work; the API, PostgreSQL, and Restate worker.

## Constraints

- Builds on the timed publishing and recovery in [intent 00003](../00003-scheduling-recovery/intent.md).
- PostgreSQL owns slots and schedules; Restate holds execution state only, per [infrastructure D4](../00001-infrastructure/spec.md).
- A fixed time must not silently move when posting slots change.

## Open questions

- Does a queued publication keep a fixed time when it enters the queue, or move when slots and earlier work change?
- How are missing and repeated local slot times resolved?
- What happens to an unclaimed queue when its channel cannot publish?
