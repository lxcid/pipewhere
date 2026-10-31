---
status: draft
---

# Intent 00010 — Agent access over MCP

## Problem

Operators want agents to draft and queue publications for them. An agent works through tool calls that can time out and be retried. A retried call that creates a second publication posts twice. An agent that reports an unknown outcome as success misleads the operator who trusted it.

An agent needs its own credential, scoped to one organization and revocable on its own. The operator needs to see what each agent did.

## Proposed outcome

An agent connected over MCP with an organization's API key can:

- list channels and their status
- create publications, and publish them now or pin a time
- read each publication's state and its attempts

Every create and retry carries an `Idempotency-Key` that the tool supplies, so a retried tool call returns the first result. Tool results use the API's names and states. Each publication records which actor created it. Revoking the key stops the agent on its next request.

## Affected users and systems

Operators running agents; the MCP server, API, and Auth.

## Constraints

- The MCP server uses the same API as every other client, per [infrastructure intent](../00001-infrastructure/intent.md). It does not add a fifth application service to [infrastructure D1](../00001-infrastructure/spec.md).
- API keys belong to an organization, not to the member who created them, per infrastructure D2.
- Create and retry requests follow [publication contract D5](../00002-publication-contract/spec.md).
- A CLI is out of scope; it needs its own intent.

## Open questions

- Before [team review](../00006-team-review/intent.md) exists, may an agent publish without review, or only create publications that wait for a person?
