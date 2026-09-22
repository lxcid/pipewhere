---
status: draft
---

# Intent 00001 - Self-hostable infrastructure foundation

## Problem

Pipewhere must support both one operator on one box and a hosted service with many workspaces. Infrastructure chosen for scale usually adds queues, caches, workers, clusters, or managed services. Each addition makes the small deployment harder to run, expose, back up, upgrade, and debug.

Choosing only for the single box creates the opposite problem: the topology must be rewritten when load arrives. Pipewhere needs one shape that runs on a laptop and scales by changing where components run, not by changing the system operators must understand.

This choice cannot be deferred safely. Deployment documentation, configuration, runbooks, and state ownership all depend on the topology. Retrofitting it later would be expensive and could leave the product unable to meet its self-hosting or scale requirements.

## Proposed outcome

Pipewhere has three infrastructure components:

- **PostgreSQL** stores business state.
- **RustFS** provides S3-compatible object storage.
- **Restate** provides durable execution for long-running and scheduled work.

Four application services run on that infrastructure:

- **Web** is a Next.js frontend that can run on edge infrastructure, which is a hard requirement.
- **Auth** is a Hono server built with Better Auth. It handles authentication and authorization, and issues bearer tokens.
- **API** is the public Rust service. It accepts bearer tokens and gives every client the same product interface.
- **Worker** is a Rust service containing the Restate handlers for scheduled and long-running work.

Most of these services are small and narrowly scoped. Rust keeps the API and worker efficient, leaving more of the machine available to PostgreSQL, RustFS, and Restate. Separating auth from the API also preserves an API-first design: web, REST, MCP, and CLI clients all use the same API instead of depending on frontend-specific behavior.

Running `docker compose up` brings up a complete Pipewhere instance that is ready to use and can be left running on an entry-level MacBook Pro without getting in the way of normal development work. The same components can move from one machine to a hosted deployment without changing the system's architecture.

## Affected users and systems

- Self-hosting operators, who need the full stack to fit comfortably on one machine.
- Hosted operators, who need the same stack to scale across machines.
- Web, REST, MCP, and CLI clients, which share the same authenticated API.

## Constraints

- The stack is fixed: Rust, Next.js, Better Auth, Restate, PostgreSQL, and RustFS.
- The frontend must be deployable on edge infrastructure.
- Authentication and authorization must remain separate from the product API, with bearer tokens as their primary boundary.
- The complete default deployment must remain responsive on an entry-level MacBook Pro while leaving enough capacity for normal development work.
- Scaling may move components to separate machines, but must not require a different architecture.

## Open questions

None.
