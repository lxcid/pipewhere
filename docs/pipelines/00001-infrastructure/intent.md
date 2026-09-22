---
status: in-progress
---

# Intent 00001 — Self-hostable infrastructure foundation

## Problem

Pipewhere has no substrate. There is no way to run it, and therefore no way to find out whether the architecture we are describing actually holds together.

The deeper problem is ordering. Self-hosting is a primary product constraint, not a packaging step at the end. Every service in the topology is something an operator runs, exposes, backs up, upgrades, and debugs at 2am. Choosing the domain model first and discovering the operational shape later means meeting operational constraints when they are most expensive to honour, or quietly failing to meet them at all.

Restate makes this sharper. It is a second stateful store next to Postgres. Whether that costs an operator one backup or two is an architectural decision, and it has to be made before the code assumes an answer.

## Proposed outcome

One command brings up a working Pipewhere.

- `docker compose up` starts the dependency stack and the application together.
- The application migrates its own schema, registers itself with Restate, and answers a readiness check that fails when a dependency is missing.
- No manual registration step. No CLI for an operator to install. No hand-written bootstrap sequence in a quickstart.
- The topology is documented, including which stores need backing up and what is lost if each one disappears.

Success is an operator who has never seen this repository going from clone to a running instance without reading anything but the README.

## Affected users and systems

- **Operators self-hosting Pipewhere.** The primary audience for this pipeline. The service count and the backup story are their inherited cost.
- **Mempipe**, as the first real operator and the dogfooding target. It already runs the same Postgres, Restate, and object-store shape, so the operational pattern is not novel here.
- **Every client** — REST, MCP, CLI, web — depends on the API being reachable and on the application having successfully registered its Restate handlers at boot.
- **Restate**, which must be able to reach the application's handler endpoint, and whose deployment registration is not idempotent.

## Constraints

- Stack is fixed: Rust, Next.js, Restate, Postgres, S3-compatible storage.
- Self-hosting is a primary constraint. The number of services an operator runs is a product metric.
- Valkey is optional and must never be required for durable correctness.
- Licensed under Elastic License 2.0. Source-available, self-hosting permitted, offering Pipewhere as a managed service is not.
- Postgres is the source of truth for business state.
- Restate's Rust SDK is pre-1.0 while the server is 1.7.x.
- Secrets never have defaults that let the process start in a broken state.

## Open questions

- **Next.js as a static export, or with a Node runtime?** Static export gives four services instead of five and no reverse proxy, and makes "the web client holds no business logic" structural. It costs SSR, server components, route handlers, and server actions. Needs an operator decision.
- **MinIO or Garage as the default object store?** MinIO's Docker Hub repository is no longer pullable and its newest community release is roughly a year old. Garage is actively maintained and built for self-hosting, at the cost of a longer init sequence. Not architectural — the application speaks S3 either way — but it is what an operator inherits.
- Does the Restate SDK's ingress client expose an idempotency key on invocation? The reconciliation sweeper's safety depends on it.
- Does a single-node Restate deployment gain anything from snapshots, or is the persistent volume the whole durability story?
- The brief positions Pipewhere as open source. Elastic License 2.0 is source-available. The positioning language needs settling before it reaches a README or a landing page.
