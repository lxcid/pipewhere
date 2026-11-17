# Plan 00011 — Publishing to one Mastodon account from the web UI

Built in one PR, against #41. API and Worker do not change. Every endpoint Web needs already exists: create, list, cancel, and the attempt list.

## Shape

- `apps/web/app/publishing.ts` holds the logic, with no React: the idempotency key, the submission state (D1), reading a local time (D2), the list's order (D3), merging a refreshed page, what each state means, and what to say about a refused or unconfirmed request.
- `apps/web/app/publications.tsx` is the organization page's Publications card. It has the form, the chronological list, the newest attempt's reason (D5), and cancel (D6). It refreshes while work can still change (D4).
- The organization page shows the card under Channels. The page renders only the organization its address names, and drops an answer for one it has moved on from.
- `useApi` passes extra headers, for `Idempotency-Key`.

## Steps

1. Tests for `publishing.ts`, then the module. Done.
2. The Publications card, and its place on the organization page. Done.
3. The organization page guard against stale answers. Done.
4. Check every interaction in a browser against a local stack, and capture screenshots. In progress.

## Verification

### Commands

Run at the PR's head, from the repository root unless noted:

- `pnpm --filter @pipewhere/web test`: the tests in `publishing.test.ts`.
- `pnpm --filter @pipewhere/web typecheck`
- `pnpm --filter @pipewhere/web build`
- `pnpm fmt:check`. It also reports three files that `main` already leaves unformatted. This change does not touch them.

API, Worker, and Auth do not change, so their suites are not part of this change's verification.

### Browser checks against a local stack

The stack runs only on this machine, with its own PostgreSQL and Restate containers. API and Worker run from their libraries with the plain-HTTP Mastodon client, against a fake Mastodon instance on a local port. Its account is `@fixture-demo`. No real instance is called.

Pending: the results of each check below.

- Publish now, and the list showing the post go out.
- Schedule for a time, and the time shown in the browser's zone.
- A double click sending one request.
- A request that gets no answer, then "Send again" answering with the same publication.
- A refused text.
- An instance refusal, shown as failed with the instance's reason.
- A server error at the instance, shown as outcome unknown with its reason.
- Cancel, and a cancel that arrives after the publication has started.
- Moving between organizations while an organization's answer is delayed.
