---
status: in-progress
---

# Intent 00011 — Publishing to one Mastodon account from the web UI

## Problem

The operator writes and schedules their own posts. They chose to do it in the web UI before an agent does it for them, per #41. The web UI can connect a Mastodon account, but it cannot write, schedule, or check a publication. Today all of that takes the API.

Draft [intent 00009](../00009-web-publishing/intent.md) describes publishing in the web UI in full. This intent splits off the first part the operator needs, so it can be approved and built while the rest of 00009 stays a draft.

## Proposed outcome

In the web UI, with one connected Mastodon account, an operator can:

- write the text of a publication
- publish it now, or schedule it for a fixed time
- see each publication's state in a chronological list
- see why a publication failed, and when its outcome is unknown
- cancel a publication that is still waiting

This is true when an operator who does not use the API can do each of these from their organization's page, and the list shows the state the API reports.

## Affected users and systems

The operator; Web, through the API.

## Constraints

- Web calls the same API as every other client and does not become a privileged backend, per [infrastructure D2](../00001-infrastructure/spec.md). Web stays edge-deployable, per infrastructure D3.
- Web uses the names and states of [publication contract D1](../00002-publication-contract/spec.md), and never shows `outcome_unknown` as failed or published.
- A repeated or resent submission must not create a second publication, per publication contract D5.
- The list is chronological. A calendar view is not needed.
- Media, several channels, and retry controls are out of scope. The rest of 00009's outcome stays with 00009.
- No test or demonstration sends a real post.

## Open questions

None.
