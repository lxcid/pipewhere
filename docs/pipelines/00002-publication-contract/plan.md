# Plan 00002 — Publication contract

The intent asks for a contract, not an implementation. The build is one document, `apps/api/openapi.yaml`, plus the decisions that shaped it. D8 to D13 record those decisions.

## Steps

1. **Write the schema.** Built. It describes every entity, state, transition, and error, and the two endpoints API serves today.
2. **Lint it.** Built. `pnpm api:lint` runs Redocly CLI 2.64.1 with its strict ruleset, configured in `redocly.yaml`.
3. **Check it against the intent.** Done below.

## What was built

- `apps/api/openapi.yaml`, an OpenAPI 3.1 document:
  - `GET /health` and `GET /v1/orgs/{organization_id}`, which API serves.
  - Channels: list and read.
  - Content: create, list, read, and edit.
  - Publications: create, list with filters, read, edit, cancel, retry, resolve, and list attempts.
  - Every endpoint except `GET /health` and `GET /v1/orgs/{organization_id}` is marked `x-pipewhere-status: planned`.
- `redocly.yaml`, which extends Redocly's `recommended-strict` and turns off two rules, each with its reason.
- `pnpm api:lint` in the root `package.json`.

Choices made in the build, within the spec:

- Lists are paged by an opaque cursor, at most 100 items a page. Publications and content list newest first, channels oldest first.
- `limit` defaults to 50.
- A publication's `body` has no length limit in the schema. Each provider has its own, and the provider pipelines check them.
- A request body rejects fields the schema does not list.

## Verified against the intent

Run on 2026-11-02.

`pnpm api:lint` passes with no warnings. It fails on a broken `$ref` and on a misspelled keyword, each tried once and reverted.

Each condition in the intent's proposed outcome, and where the schema meets it:

- **One name and one meaning for each entity and state, including an attempt record.** The schema's description defines the five entities, in D1's words. `PublicationState` defines each state, and `PublicationAttempt` its results, per D11.
- **Each channel's publication shows exactly what goes out, and source edits cannot change it.** `Publication.body` is the publication's own copy, per D2. `updateContent` says it never edits a publication. `Publication` says its body and remote reference never change once `published`.
- **Transitions distinguish work safe to repeat from a commit whose outcome may be unknown.** `PublicationState` lists D3's transitions. `publishing` to `preparing` is the safe rerun, and `outcome_unknown` is never retried automatically. `retryPublication` refuses `outcome_unknown`, and `resolvePublication` is the only way out of it.
- **Repeating a request returns the first result, and an ambiguous result is never committed again automatically.** `createPublication` and `retryPublication` require `Idempotency-Key`, whose description carries D5's rules. `outcome_unknown` and `resolvePublication` carry D4's.
- **Every publication belongs to one organization, and a single operator needs no team setup.** Every business path is under `/v1/orgs/{organization_id}`, per D6, and nothing in the schema refers to members or roles. A user creates an organization in Web and is its only member; nothing more is needed.

`GET /v1/orgs/{organization_id}` and the error body were compared with `apps/api/src/lib.rs`, `access.rs`, and `error.rs` by reading them. The schema adds the optional `field` and `state` to the error body, which API does not send yet.

## Not verified

- That API's served responses match the schema, beyond reading the code. D8 assigns a response test to the first pipeline that implements a planned endpoint.
- That the contract suits the clients that will use it. No client calls these endpoints yet.
