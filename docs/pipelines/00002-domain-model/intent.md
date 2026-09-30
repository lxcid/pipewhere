---
status: draft
---

# Intent 00002 — Publication contract

## Problem

One piece of writing can go to several channels with different outcomes. A scheduled copy must not change when its source is edited. After a provider timeout, Pipewhere may not know whether a post went live. Treating that outcome as a failure can create a duplicate; treating it as success can hide a missed post. Clients can also retry a request after losing its response and create the same publication twice.

Later API, workflow, and client pipelines need one small contract for these facts before they choose scheduling and provider mechanics.

## Proposed outcome

Define the publication entities, states, and transitions in the REST API schema. The contract lets an operator answer, for each channel: what is intended to go out, what went out, what failed and why, or whether the outcome is unknown. A publication's sent record is immutable. An ambiguous provider result stays unknown until a person resolves it. Repeating a client request does not create another publication.

The contract is ready for implementation when:

- Each entity and state has one name and one meaning, including an attempt record that explains failures and unknown outcomes.
- A publication holds its own body for one channel. Changing source content cannot change that publication.
- The allowed transitions distinguish work that is safe to repeat from a provider commit whose outcome may be unknown.
- The published API contract requires a stable client retry key and forbids an automatic second provider commit after an ambiguous result.
- Every publication belongs to one organization. A single operator can use an organization without a team setup step.

This pipeline defines the contract. Later pipelines implement scheduling, provider adapters, and interfaces against it.

## Affected users and systems

Operators reading publication outcomes; API clients retrying requests; the REST API, database, and worker that will implement the contract; later MCP, CLI, and web clients that will use its vocabulary.

## Constraints

- PostgreSQL owns business records and Restate holds execution state only, per [infrastructure D4](../00001-infrastructure/spec.md).
- Auth owns organizations, members, roles, and API keys, per [infrastructure D2](../00001-infrastructure/spec.md). New ids follow [infrastructure D6](../00001-infrastructure/spec.md).
- A duplicate public post is harder to recover from than a missed post. The contract must preserve an unknown outcome instead of guessing.
- Nothing becomes public except by publishing to a channel. There is no generic public/private flag.
- The first provider adapters will be Mastodon and Threads. Their live behavior and commit mechanics are decided and verified in their implementation pipelines.
- Organization and channel identities must work in a local deployment and in a later hosted one.

Queue slots, grace periods, media, channel capabilities and credentials, team permissions, approval policy, and the provider adapters are outside this contract. They need separate intents before implementation. The draft proposals removed from this pipeline remain in this branch's history for those later reviews; they are not approved decisions.

## Open questions

None for this contract. Provider behavior, timing, and review questions belong to the later intents that own them.
