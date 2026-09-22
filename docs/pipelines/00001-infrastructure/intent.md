---
status: in-progress
---

# Intent 00001 — Self-hostable infrastructure foundation

## Problem

The infrastructure has to carry one operator on one box and a hosted service with many workspaces, and it gets chosen once. Those two cases pull against each other. The usual answers to scale — a queue, a cache, a worker tier, a cluster, a managed service per concern — each add something an operator has to run, expose, back up, upgrade and debug at 2am. Every one of them improves the large deployment by degrading the small one.

Both common failures follow from treating that as unavoidable. Pick for scale and the self-hosted install inherits machinery it never needed. Pick for the single box and the topology gets rewritten the first time load arrives. What is wanted instead is a set of components where the same shape runs on a laptop and scales out, so growing changes where things run rather than what the operator has to understand.

Deferring the choice does not avoid it. Topology is the most expensive thing to change later: the deployment documentation, the configuration surface, the runbook, and every decision about where state lives all assume it. Choosing the domain model first and discovering the operational shape afterwards means meeting these constraints when they cost the most to honour, or quietly failing to meet them.

Restate makes the tension concrete. It is what scales the execution layer out, and it is a second stateful store beside Postgres. Whether that costs an operator one backup or two is an architectural decision, and the answer has to exist before code assumes one.

## Proposed outcome

One command brings up a working Pipewhere, and the same shape scales out without becoming a different system.

- `docker compose up` starts the dependency stack and the application together.
- The application migrates its own schema, registers itself with Restate, and answers a readiness check that fails when a dependency is missing.
- No manual registration step. No CLI for an operator to install. No hand-written bootstrap sequence in a quickstart.
- The topology is documented, including which stores need backing up and what is lost if each one disappears.
- Every component scales in place rather than being swapped out. Postgres becomes managed Postgres, a single Restate node becomes a cluster, MinIO becomes S3 or R2, one application process becomes several. The operator learns one system either way.
- Nothing the large deployment wants is forced on the small one. Valkey is the test case: worth having as a cache under load, never required for durable correctness.

Success has two tests. An operator who has never seen this repository goes from clone to a running instance reading only the README. And moving from that instance to a hosted deployment changes configuration, not architecture.

## Affected users and systems

- **Operators self-hosting Pipewhere.** The primary audience for this pipeline. The service count and the backup story are their inherited cost.
- **Mempipe**, as the first real operator and the dogfooding target. It already runs the same Postgres, Restate, and object-store shape, so the operational pattern is not novel here.
- **Every client** — REST, MCP, CLI, web — depends on the API being reachable and on the application having successfully registered its Restate handlers at boot.
- **Restate**, which must be able to reach the application's handler endpoint, and whose deployment registration is not idempotent.

## Constraints

- Stack is fixed: Rust, Next.js, Restate, Postgres, S3-compatible storage.
- Self-hosting is a primary constraint. The number of services an operator runs is a product metric.
- Scale is equally primary. The same topology serves one operator and many workspaces; scaling may move components, but it may not add ones the small install has to run.
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
