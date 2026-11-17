# Plan 00011 — Publishing to one Mastodon account from the web UI

Built in one PR, against #41. API and Worker do not change. Every endpoint Web needs already exists: create, list, cancel, and the attempt list.

## Shape

- `apps/web/app/publishing.ts` holds the logic, with no React: the idempotency key, the submission state (D1), reading a local time (D2), the list's order (D3), merging a refreshed page, what each state means, and what to say about a refused or unconfirmed request.
- `apps/web/app/api.ts` holds the request to API, also without React. An answer whose body is not a JSON object throws a plain error, so a cut-off 2xx counts as unconfirmed, not created. `useApi` wraps it.
- `apps/web/app/publications.tsx` is the organization page's Publications card. It has the form, the chronological list, the newest attempt's reason (D5), and cancel (D6). It refreshes while work can still change (D4).
- The organization page shows the card under Channels. The page renders only the organization its address names, and drops an answer for one it has moved on from. Next.js 16.4.2 already mounts a new page for each organization id, because its layout router keys a segment by its value. The guard keeps the page correct without relying on that.

## Steps

1. Tests for `publishing.ts`, then the module. Done.
2. The Publications card, and its place on the organization page. Done.
3. The organization page guard against stale answers. Done.
4. Tests for an unreadable successful answer, then `api.ts`. Done.
5. Check every interaction in a browser against a local stack, and capture screenshots. Done.

## Verification

### Commands

Run at `0169f7d`, from the repository root unless noted:

- `pnpm --filter @pipewhere/web test`: 23 pass. `publishing.test.ts` covers the key, the submission state, local times in New York including both daylight-saving changes, the list's order, merging, the state descriptions, and the messages. `api.test.ts` runs the real request code against a stubbed `fetch`: an unreadable 2xx, a 5xx, a failed connection, a 4xx, and a create whose answer is cut off after API stored it, sent again with the same key.
- `pnpm --filter @pipewhere/web typecheck`: clean.
- `pnpm --filter @pipewhere/web build`: passes. In an agent's shell, Next.js 16.4.2 first stops with a notice that this version has a known security advisory, and passes when run again. The notice is about the installed Next.js, not this change.
- `pnpm fmt:check`: reports only three files that `main` already leaves unformatted. This change does not touch them.

API, Worker, and Auth do not change, so their suites were not run.

### Browser checks against a local stack

Verified at `0169f7d`. The stack ran only on the builder's machine, with its own PostgreSQL and Restate containers. Web ran as a production build. API and Worker ran from their libraries with the plain-HTTP Mastodon client, against a fake Mastodon instance on a local port. The fake's account is `@fixture-demo`, and every post text starts with "Fixture". No real instance was called. A script drove headless Chrome 157 through the DevTools protocol, and read the database to check each result. It simulated network failures by intercepting requests in the browser.

All 33 checks passed:

- Without an active channel, the card asks to connect one. Connecting the fake account shows the form.
- Publish now with a double click sends one request and creates one publication. Within the list's refresh, it shows as published, with the post's address.
- Scheduling in Asia/Singapore names the zone, previews the time, and stores the same instant in UTC.
- A time in the past is refused before sending.
- In America/New_York, 2:30 on 2027-03-14, which the clock skips, previews as 3:30 GMT-4.
- The instance's 422 shows as failed, with the instance's reason.
- The instance's 500 shows as outcome unknown, with its reason and no cancel. 15 seconds later it still has one attempt.
- Cancel asks to confirm, then cancels.
- A cancel that arrives after the publication moved to `publishing` shows that it could no longer be cancelled, and the list shows `publishing`. The move was made in the database, standing in for Worker's claim.
- A 201 whose body is cut off: API stored the publication, the form locks and says the request was not confirmed, and "Send again" reuses the key and finds the one publication.
- A request that never reaches API: nothing is stored, and "Send again" reuses the key and creates it once.
- A 422 from API keeps the form editable, and the next submission takes a new key.
- While the first organization's answer is held, its page shows only "Loading…". After moving to a second organization and then releasing the held answer, the page still shows the second organization, without the first's form.
- A failed list load says so, and Refresh recovers.
- With 101 more publications, the first page shows 100 and "Show older", which loads the rest.
- No sideways scroll at 390 pixels wide. No console errors.

Screenshots from that run, all of fixture data:

- [The form, with the fake account connected](evidence/compose.png)
- [Scheduling in Asia/Singapore, with the preview](evidence/schedule-preview.png)
- [The daylight-saving gap in America/New_York](evidence/daylight-saving-gap.png)
- [Failed and outcome unknown, each with its reason](evidence/failed-and-unknown.png)
- [An unconfirmed create, and a cancel that lost the race](evidence/unconfirmed-and-cancel-race.png)
- [The second organization, after the first's late answer](evidence/organization-switch.png)
- [Phone width](evidence/phone.png)
- [Dark scheme](evidence/dark-scheme.png)

### Not verified

- A real Mastodon instance. The fake follows the answers the API's own test fake uses.
- Auth refusing a token mid-session. That error is unconfirmed, like any other request that is not answered.
- Browsers other than Chrome.
