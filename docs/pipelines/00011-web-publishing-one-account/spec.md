# Spec 00011 — Publishing to one Mastodon account from the web UI

## Decisions

### D1 — A submission keeps its `Idempotency-Key` until API says whether it took effect

Proposed: awaiting operator approval.

[Publication contract D5](../00002-publication-contract/spec.md) asks a client to retry with the same key, and D13 says how API answers it. Web's form meets that as follows:

- **One key per submission.** Web makes the key from 16 random bytes in base64url, 22 characters, when the operator submits. A second submit while the first is unanswered sends nothing.
- **Created.** A 201 ends the key. The form clears, and the next submission is a new publication with a new key.
- **Refused.** A 4xx created nothing, because API stores nothing for a refused request. The form shows why and stays editable. The next submission takes a new key.
- **Unconfirmed.** A request with no answer, or a 5xx, may have created the publication. Web keeps the key and the request, and locks the form. It never shows this as success. "Send again" sends the same request with the same key, so API answers with the first publication if there is one. "Discard" drops the key and unlocks the form, with a warning that sending the text again can post it twice.
- **Publish now leaves out `planned_at`.** API then sets the time by its own clock, and a resent request hashes the same as the first.

The key lives in the page's memory. Reloading the page loses it. Tolerated: after a reload the list shows whether the publication exists, so the operator can see it before writing the text again. Keeping the key in browser storage would add a store, an expiry rule, and a cleanup path to save a check the list already gives.

### D2 — The operator enters times in the browser's time zone, and Web sends them in UTC

Proposed: awaiting operator approval.

The form names the browser's time zone beside a `datetime-local` input. Web reads the value in that zone and sends it as an RFC 3339 time in UTC, which API's `planned_at` accepts.

- A time the clock skips at a daylight-saving change moves forward by the skipped hour. A time the clock repeats takes its first occurrence. That is how the JavaScript `Date` constructor reads local fields. The form shows the resolved time before the operator schedules, so the move is visible.
- A time that is not in the future is refused before sending. The operator chose "publish now" for that case.
- A year past 9999 is refused before sending, as API would refuse it.
- The list shows every time in the browser's zone, with the zone's short name.

A zone picker was not built. The operator schedules from one browser, in their own zone.

### D3 — The chronological list puts the latest planned time first

Proposed: awaiting operator approval.

The intent asks for a chronological list. Web orders it by planned time, latest first. Of two publications at one time, the newer comes first. A list by planned time puts what goes out next beside what went out last, which is what a person checking their schedule looks for. Creation order would put a post written today for next week above one written yesterday for this afternoon.

API lists publications newest first by creation, 100 to a page, per publication contract D14. Web orders the pages it has loaded, and "Show older" loads the next one. With 100 publications or fewer, the order is exact. With more, a publication created before the latest 100 but planned later appears only after its page loads.

Tolerated for one operator's volume. An API order by planned time would remove it. That is a change to API's list, which this slice does not make.

### D4 — The list refreshes itself while any publication can still change on its own

Proposed: awaiting operator approval.

While any loaded publication is `scheduled`, `preparing`, or `publishing`, Web reloads the first page and the channels every 10 seconds. A Refresh button does the same at any time. A refreshed page replaces the publications it holds and keeps older pages already loaded.

Worker looks for due work every 5 seconds, so a publication sent now shows its outcome within one or two refreshes. No background work runs while every publication has settled.

### D5 — A failure's reason comes from the newest attempt

Proposed: awaiting operator approval.

A `failed` or `outcome_unknown` publication holds no reason. Its newest attempt does, per [publication contract D19 and D20](../00002-publication-contract/spec.md). Web loads the attempts of each such publication and shows the newest attempt's reason. It loads them again when the publication's state changes.

That is one request for each failed or unknown publication on screen. Tolerated for one operator, whose failures are few.

### D6 — Cancel needs a second click, and a refused cancel shows the current state

Proposed: awaiting operator approval.

Web offers cancel for `scheduled` and `preparing`, the states [publication contract D3](../00002-publication-contract/spec.md) allows. A cancelled publication cannot be resumed, so the first click asks the operator to confirm.

If Worker moves the publication to `publishing` before the cancel arrives, API answers 409 `state_conflict`. Web says it could no longer be cancelled and reloads the list to show where it is. A cancel with no answer is not shown as cancelled either: Web says the request was not confirmed and reloads the list.

### D7 — Web's logic is tested with Node's own test runner

Proposed: awaiting operator approval.

Web had no tests. Its new logic lives in `publishing.ts`, which imports nothing from React. Node 26 runs TypeScript test files by removing their types, so `node --test` runs that module's tests without a test framework. `tsconfig.json` allows the `.ts` import the test needs.

The React components hold no decisions of their own. They are checked by running Web against a local stack in a browser, as the plan records.

What would add component tests: logic moving into a component, or a bug that the module's tests and the browser check both missed.

### D8 — The text is not checked against the instance's limit before it is scheduled

Proposed: awaiting operator approval.

API has no endpoint that checks a text against a channel's limits, and it creates publications without that check. Mastodon refuses text over its limit when the post is sent. The publication then fails with the instance's reason, which the list shows.

Web shows no character count. Mastodon counts each link as 23 characters, so a plain count would mislead.

[Intent 00009](../00009-web-publishing/intent.md) asks to "see, before anything is created, any reason the API gives for refusing the text for a channel". That stays with 00009, and it needs an API check first.
