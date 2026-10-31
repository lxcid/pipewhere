---
status: draft
---

# Intent 00005 — Media and channel limits

## Problem

A publication accepted by one channel may exceed another channel's text or media limits. Each Mastodon instance sets its own limits. Discovering a known limit only at publish time turns a request error into a missed scheduled post. Both first providers process media asynchronously, and Threads fetches it from a URL.

Providers also count text differently. A count that differs from the provider's, for example on links or non-Latin scripts, either rejects text the provider would accept or accepts text it will refuse.

## Proposed outcome

The API validates publication text and media against each channel's known limits when it is created, edited, retried, or approved. Text length is counted the way each provider counts it. A request with an invalid target creates nothing and explains each target's problem. Media used by a publication stays a stable snapshot. Missing object bytes produce a clear failure. Provider processing completes before a media publication commits.

## Affected users and systems

Operators and clients preparing posts; API, object storage, Mastodon and Threads adapters.

## Constraints

- The [publication contract](../00002-publication-contract/intent.md) keeps each channel's publication independent.
- RustFS owns object bytes and PostgreSQL owns references, per [infrastructure D4](../00001-infrastructure/spec.md).
- Provider limits may change or be unavailable; a later provider rejection remains possible.

## Open questions

- Which limit values can each provider and Mastodon instance report reliably?
- How does each provider count text length, and can Pipewhere reproduce that count before publishing?
- How does Threads fetch private media without making the whole object store public?
