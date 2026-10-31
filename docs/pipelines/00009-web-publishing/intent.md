---
status: draft
---

# Intent 00009 — Publishing in the web UI

## Problem

The web UI handles accounts, organizations, and API keys, but not publishing. An operator who does not use the API cannot write, schedule, or check publications.

An operator also needs to act on what the publication contract reports. For each channel they need to see what will go out, what went out, what failed and why, and what is unknown. An unknown outcome shown as failed invites a duplicate repost. One shown as published hides a missed post.

## Proposed outcome

In the web UI, an operator can:

- connect and reconnect channels, and see each channel's status and the reason it cannot publish
- write once and create publications for several channels, then edit each channel's copy
- see each channel's problem that the API reports before anything is created
- publish now or pin a time
- see each publication's state and planned time, and the attempts behind a failure or unknown outcome
- edit and retry a failed publication, and cancel a waiting one
- resolve an unknown outcome by giving the post's URL or by confirming that nothing was posted

## Affected users and systems

Operators and team members; Web, through the API.

## Constraints

- Web calls the same API as every other client and does not become a privileged backend, per [infrastructure D2](../00001-infrastructure/spec.md). Web stays edge-deployable, per infrastructure D3.
- Web uses the names and states of [publication contract D1](../00002-publication-contract/spec.md), and never shows `outcome_unknown` as failed or published.
- Posting slots, media, and review screens are out of scope until their pipelines ship.

## Open questions

- Does the first version need a calendar view, or is a list ordered by time enough?
