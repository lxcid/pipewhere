---
status: draft
---

# Intent 00008 — Threads delivery

## Problem

Threads publishes in two steps. A client creates a media container, then publishes it with `threads_publish`. Meta's [publishing guide](https://developers.facebook.com/docs/threads/posts) recommends waiting about 30 seconds between the two.

A container reports `IN_PROGRESS`, `FINISHED`, `PUBLISHED`, `ERROR`, or `EXPIRED`, per Meta's [troubleshooting guide](https://developers.facebook.com/docs/threads/troubleshooting). A container not published within 24 hours expires. The guide presents the status check as the fallback when `threads_publish` returns no media id. Meta does not document what a second `threads_publish` with the same container does.

A profile may publish 250 posts through the API in a moving 24-hour window. `threads_publish` enforces the cap, and `threads_publishing_limit` reports current usage.

Threads access tokens expire. A long-lived token is valid for 60 days and can be refreshed once it is at least 24 hours old. A token not refreshed within 60 days expires and can no longer be refreshed, per Meta's [long-lived token guide](https://developers.facebook.com/docs/threads/get-started/long-lived-tokens).

An app without App Review approval for `threads_content_publish` can post only for its Threads testers. A self-hosting operator running their own Meta app is limited to tester accounts until that app passes review. They need to know this before they try to connect a channel.

## Proposed outcome

- A Threads adapter publishes through the [publication contract](../00002-publication-contract/intent.md) and the channel model from [intent 00004](../00004-provider-delivery/intent.md).
- Container creation and processing happen before the commit. `threads_publish` is the commit.
- A publication refused by the daily cap fails with a reason that names the cap.
- Credential storage and refresh keep working across restarts. A connected channel's access does not lapse while the channel stays connected.
- An operator can learn, before connecting, what Meta access their deployment needs to publish to their accounts.

## Affected users and systems

Operators connecting Threads accounts; API, the Threads adapter, PostgreSQL, Restate, and Meta's Threads API.

## Constraints

- Never automatically repeat an ambiguous commit, per [publication contract D4](../00002-publication-contract/spec.md).
- Organizations that link the same Threads account share its daily cap, per [publication contract D7](../00002-publication-contract/spec.md).
- Provider secrets must not be stored in plain text.
- Threads media is fetched from a URL; [intent 00005](../00005-media-capabilities/intent.md) owns media.

## Open questions

- If `threads_publish` is re-issued with an already-published `creation_id`, does it deduplicate, and can container status settle a timed-out publish?
- Does owned-account access to Threads require full app review, or can development-mode tester access suffice?
