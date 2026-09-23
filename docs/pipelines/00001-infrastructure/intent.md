---
status: approved
---

# Intent 00001 — Self-hostable infrastructure foundation

## Problem

Pipewhere must support both one operator on one box and a hosted service with many workspaces. Infrastructure chosen for scale usually adds queues, caches, workers, clusters, or managed services. Each addition makes the small deployment harder to run, expose, back up, upgrade, and debug.

Choosing only for the single box creates the opposite problem: the topology must be rewritten when load arrives. Pipewhere needs one shape that runs on a laptop and scales by changing where components run, not by changing the system operators must understand.

This choice cannot be deferred safely. Deployment documentation, configuration, runbooks, and state ownership all depend on the topology. Retrofitting it later would be expensive and could leave the product unable to meet its self-hosting or scale requirements.

## Proposed outcome

One command brings up a working Pipewhere instance. The same shape can scale into a hosted deployment without becoming a different system.

Success has three tests:

- A new operator can go from clone to a running instance using only the README.
- The complete stack can remain running on an entry-level MacBook Pro without getting in the way of normal development work.
- Moving to a hosted deployment changes configuration and placement, not architecture.

## Affected users and systems

- Self-hosting operators, who need the complete stack to fit comfortably on one machine.
- Hosted operators, who need the same system to scale across machines.
- Web, REST, MCP, and CLI clients, which must share one authenticated API.

## Constraints

The operator has fixed the following constraints:

- The three infrastructure components are PostgreSQL, RustFS, and Restate.
- The four application services are Web, Auth, API, and Worker.
- API and Worker are Rust services. Worker hosts the Restate handlers.
- Web uses Next.js and remains independently deployable to edge infrastructure.
- Auth is a separate Hono service built with Better Auth. It handles authentication and authorization and issues bearer tokens to API clients.
- The product is API-first. Web, REST, MCP, and CLI clients use the same API rather than frontend-specific business behavior.
- Scaling may move components to separate machines, but must not require a different architecture.

## Open questions

None.
