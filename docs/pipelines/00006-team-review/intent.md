---
status: draft
---

# Intent 00006 — Team and service-actor review

## Problem

One operator should be able to publish without extra steps. A team needs different powers for writing, publishing, connecting channels, and managing members. Agents and automations need their own credentials; some may fill queues on their own, while others need human review. Approval after a planned time must not silently publish stale work.

## Proposed outcome

An organization works with one member or several. Humans and service actors have explicit permissions. An administrator can require review for selected actors, see what is pending, and approve or reject it. An actor cannot approve its own submission. Approval that arrives too late requires a new timing choice. Removing a member or revoking an API key takes effect on the next request.

## Affected users and systems

Operators, team members, agents and automations; Auth, API, and clients showing pending work.

## Constraints

- Auth owns organizations, members, roles, invitations, and API keys, per [infrastructure D2](../00001-infrastructure/spec.md).
- The [publication contract](../00002-domain-model/intent.md) supplies the publication and organization boundaries. This pipeline may add a pre-scheduling approval state.
- A single operator must not have to configure an approval policy to publish.

## Open questions

- Which actions may a service actor perform without review, and which human roles may approve or resolve unknown outcomes?
- Does review policy apply per actor, per channel, or to each publication?
