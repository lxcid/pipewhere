import assert from "node:assert/strict";
import { describe, test } from "node:test";

import {
  Submitter,
  approvalTooLate,
  chronological,
  describeState,
  loadThrough,
  merged,
  newIdempotencyKey,
  plannedAt,
  problemOf,
  type Page,
  type Publication,
  type Sent,
} from "./publishing.ts";

// Node reads TZ again whenever it changes, so the zone tests see a fixed zone
// whatever the machine's.
process.env.TZ = "America/New_York";

function publication(fields: Partial<Publication> = {}): Publication {
  return {
    id: "pub_01",
    organization_id: "org_01",
    channel_id: "chn_01",
    content_id: "cnt_01",
    body: "Hello",
    state: "scheduled",
    planned_at: "2026-11-18T10:00:00Z",
    tolerance_seconds: 1800,
    deadline: "2026-11-18T10:30:00Z",
    remote_id: null,
    remote_url: null,
    ...fields,
  };
}

const created = publication();

describe("newIdempotencyKey", () => {
  test("holds 128 random bits in the 22 base64url characters API accepts", () => {
    const key = newIdempotencyKey();
    assert.match(key, /^[A-Za-z0-9_-]{22}$/);
    assert.notEqual(newIdempotencyKey(), key);
  });
});

describe("Submitter", () => {
  const request = { channel_id: "chn_01", body: "Hello" };

  function recorder(answers: Sent[]) {
    const calls: { key: string; request: object }[] = [];
    const send = async (key: string, sent: object): Promise<Sent> => {
      calls.push({ key, request: sent });
      const answer = answers.shift();
      if (!answer) throw new Error("no answer left");
      return answer;
    };
    return { calls, send };
  }

  test("a second submit while the first is unanswered sends nothing", async () => {
    let answer!: (sent: Sent) => void;
    const calls: string[] = [];
    const submitter = new Submitter(
      (key) =>
        new Promise<Sent>((resolve) => {
          calls.push(key);
          answer = resolve;
        }),
    );
    const first = submitter.submit(request);
    const second = submitter.submit(request);
    assert.equal(calls.length, 1);
    answer({ kind: "created", publication: created });
    assert.deepEqual(await first, { kind: "created", publication: created });
    assert.equal(await second, null);
  });

  test("an unconfirmed submission is sent again with its key and its first request", async () => {
    const { calls, send } = recorder([
      { kind: "unconfirmed", reason: "Failed to fetch" },
      { kind: "created", publication: created },
    ]);
    const submitter = new Submitter(send);
    await submitter.submit(request);
    assert.deepEqual(submitter.unconfirmed, request);
    await submitter.submit({ channel_id: "chn_01", body: "Edited meanwhile" });
    assert.equal(calls[1].key, calls[0].key);
    assert.deepEqual(calls[1].request, request);
    assert.equal(submitter.unconfirmed, null);
  });

  test("a refused submission created nothing, so the next one takes a new key", async () => {
    const { calls, send } = recorder([
      { kind: "refused", status: 422, code: "invalid_body" },
      { kind: "created", publication: created },
    ]);
    const submitter = new Submitter(send);
    await submitter.submit(request);
    assert.equal(submitter.unconfirmed, null);
    await submitter.submit(request);
    assert.notEqual(calls[1].key, calls[0].key);
  });

  test("a refused resend stays unconfirmed, because the first send may have created it", async () => {
    const { calls, send } = recorder([
      { kind: "unconfirmed", reason: "Failed to fetch" },
      { kind: "refused", status: 401, code: "unauthorized" },
      { kind: "created", publication: created },
    ]);
    const submitter = new Submitter(send);
    await submitter.submit(request);
    const refused = await submitter.submit(request);
    assert.equal(refused?.kind, "refused");
    assert.deepEqual(submitter.unconfirmed, request);
    await submitter.submit(request);
    assert.equal(calls[2].key, calls[0].key);
  });

  test("a created publication ends its key, so the next submission is a new publication", async () => {
    const { calls, send } = recorder([
      { kind: "created", publication: created },
      { kind: "created", publication: created },
    ]);
    const submitter = new Submitter(send);
    await submitter.submit(request);
    await submitter.submit(request);
    assert.notEqual(calls[1].key, calls[0].key);
  });

  test("discarding an unconfirmed submission lets the next one start over", async () => {
    const { calls, send } = recorder([
      { kind: "unconfirmed", reason: "Failed to fetch" },
      { kind: "created", publication: created },
    ]);
    const submitter = new Submitter(send);
    await submitter.submit(request);
    submitter.discard();
    assert.equal(submitter.unconfirmed, null);
    await submitter.submit({ channel_id: "chn_01", body: "Another" });
    assert.notEqual(calls[1].key, calls[0].key);
    assert.deepEqual(calls[1].request, { channel_id: "chn_01", body: "Another" });
  });
});

describe("plannedAt", () => {
  const now = new Date("2026-11-18T12:00:00Z");

  test("reads the browser's local time and sends it in UTC", () => {
    assert.deepEqual(plannedAt("2026-11-18T09:30", now), {
      plannedAt: "2026-11-18T14:30:00.000Z",
    });
  });

  test("moves a time skipped by the clock change forward, as the browser does", () => {
    // 2:30 does not exist on 2026-03-08 in New York; 3:30 EDT follows 1:59 EST.
    assert.deepEqual(plannedAt("2026-03-08T02:30", new Date("2026-01-01T00:00:00Z")), {
      plannedAt: "2026-03-08T07:30:00.000Z",
    });
  });

  test("takes the first of a time the clock change repeats", () => {
    assert.deepEqual(plannedAt("2026-11-01T01:30", new Date("2026-10-01T00:00:00Z")), {
      plannedAt: "2026-11-01T05:30:00.000Z",
    });
  });

  test("refuses an empty, unreadable, past, or out-of-range time", () => {
    for (const local of ["", "soon", "2026-11-18T07:00", "2026-11-18T06:59", "10000-01-01T00:00"]) {
      assert.ok("problem" in plannedAt(local, now), local);
    }
  });
});

describe("chronological", () => {
  test("puts the latest planned time first, and the newer of two at one time", () => {
    const list = chronological([
      publication({ id: "pub_01", planned_at: "2026-11-18T10:00:00Z" }),
      publication({ id: "pub_03", planned_at: "2026-11-20T10:00:00Z" }),
      publication({ id: "pub_02", planned_at: "2026-11-18T10:00:00Z" }),
      publication({ id: "pub_04", planned_at: "2026-11-17T10:00:00Z" }),
    ]);
    assert.deepEqual(
      list.map((item) => item.id),
      ["pub_03", "pub_02", "pub_01", "pub_04"],
    );
  });
});

describe("loadThrough", () => {
  // API's list: newest id first, `id < cursor`, here two to a page.
  function api(ids: string[]) {
    const requests: (string | null)[] = [];
    const fetchPage = async (cursor: string | null): Promise<Page> => {
      requests.push(cursor);
      const rest = ids.filter((id) => cursor === null || id < cursor);
      const page = rest.slice(0, 2);
      return {
        publications: page.map((id) => publication({ id })),
        next_cursor: rest.length > 2 ? page[page.length - 1] : null,
      };
    };
    return { requests, fetchPage };
  }
  const ids = ["pub_06", "pub_05", "pub_04", "pub_03", "pub_02", "pub_01"];

  test("loads only the first page before anything older is shown", async () => {
    const { requests, fetchPage } = api(ids);
    const loaded = await loadThrough(fetchPage, undefined);
    assert.deepEqual(requests, [null]);
    assert.deepEqual(
      loaded.publications.map((item) => item.id),
      ["pub_06", "pub_05"],
    );
    assert.equal(loaded.older, "pub_05");
  });

  test("reloads every page already shown, even after newer publications push them down", async () => {
    const { requests, fetchPage } = api(["pub_08", "pub_07", ...ids]);
    const loaded = await loadThrough(fetchPage, "pub_03");
    assert.deepEqual(requests, [null, "pub_07", "pub_05"]);
    assert.deepEqual(
      loaded.publications.map((item) => item.id),
      ["pub_08", "pub_07", "pub_06", "pub_05", "pub_04", "pub_03"],
    );
    assert.equal(loaded.older, "pub_03");
  });

  test("stops at the oldest page shown, and reloads to the end once every page was shown", async () => {
    const shown = api(ids);
    assert.equal((await loadThrough(shown.fetchPage, "pub_03")).older, "pub_03");
    assert.deepEqual(shown.requests, [null, "pub_05"]);
    const everything = api(ids);
    assert.equal((await loadThrough(everything.fetchPage, null)).publications.length, 6);
    assert.deepEqual(everything.requests, [null, "pub_05", "pub_03"]);
  });
});

describe("merged", () => {
  test("replaces what was loaded with the newer answer, and keeps older pages", () => {
    const loaded = [
      publication({ id: "pub_02", state: "scheduled" }),
      publication({ id: "pub_01", state: "published" }),
    ];
    const fresh = [
      publication({ id: "pub_03" }),
      publication({ id: "pub_02", state: "outcome_unknown" }),
    ];
    const result = merged(loaded, fresh);
    assert.deepEqual(
      result.map((item) => [item.id, item.state]),
      [
        ["pub_03", "scheduled"],
        ["pub_02", "outcome_unknown"],
        ["pub_01", "published"],
      ],
    );
  });
});

describe("describeState", () => {
  const now = new Date("2026-11-18T12:00:00Z");

  test("never reports an unknown outcome as failed or published", () => {
    const view = describeState(publication({ state: "outcome_unknown" }), now);
    assert.equal(view.label, "Outcome unknown");
    assert.equal(view.tone, "unknown");
    assert.match(view.detail ?? "", /may be on Mastodon/);
    assert.match(view.detail ?? "", /will not send it again/);
    assert.equal(view.cancellable, false);
  });

  const STATES = [
    "awaiting_approval",
    "scheduled",
    "preparing",
    "publishing",
    "published",
    "failed",
    "outcome_unknown",
    "cancelled",
  ] as const;

  test("offers cancel only before the post may have been sent", () => {
    const cancellable = (state: Publication["state"]) =>
      describeState(publication({ state, planned_at: "2026-11-19T00:00:00Z" }), now).cancellable;
    assert.deepEqual(STATES.filter(cancellable), ["awaiting_approval", "scheduled", "preparing"]);
  });

  test("offers approval only while a publication awaits it", () => {
    const approvable = (state: Publication["state"]) =>
      describeState(publication({ state }), now).approvable;
    assert.deepEqual(STATES.filter(approvable), ["awaiting_approval"]);
  });

  test("says an agent's publication goes out only once approved", () => {
    const view = describeState(
      publication({
        state: "awaiting_approval",
        planned_at: "2026-11-19T00:00:00Z",
        deadline: "2026-11-19T00:30:00Z",
      }),
      now,
    );
    assert.equal(view.label, "Awaiting approval");
    assert.match(view.detail ?? "", /only after/);
  });

  test("says an agent's publication past its time goes out only if it starts by its deadline", () => {
    const view = describeState(
      publication({
        state: "awaiting_approval",
        planned_at: "2026-11-18T11:50:00Z",
        deadline: "2026-11-18T12:20:00Z",
      }),
      now,
    );
    assert.match(view.detail ?? "", /if it starts by/);
  });

  test("calls an approval too late once the deadline has passed", () => {
    const waiting = publication({ state: "awaiting_approval", deadline: "2026-11-18T12:00:00Z" });
    assert.equal(approvalTooLate(waiting, new Date("2026-11-18T11:59:59Z")), false);
    assert.equal(approvalTooLate(waiting, new Date("2026-11-18T12:00:00Z")), true);
  });

  test("warns that approving a publication past its deadline does not post it", () => {
    const view = describeState(
      publication({
        state: "awaiting_approval",
        planned_at: "2026-11-18T11:00:00Z",
        deadline: "2026-11-18T11:30:00Z",
      }),
      now,
    );
    assert.match(view.detail ?? "", /fails without posting/);
  });

  test("says a scheduled publication past its time is waiting, and until when", () => {
    const view = describeState(
      publication({ planned_at: "2026-11-18T11:59:00Z", deadline: "2026-11-18T12:29:00Z" }),
      now,
    );
    assert.equal(view.label, "Scheduled");
    assert.match(view.detail ?? "", /Waiting to start/);
  });
});

describe("problemOf", () => {
  test("explains a refused text or time", () => {
    assert.match(problemOf({ kind: "refused", status: 422, code: "invalid_body" }), /text/);
  });

  test("tells an operator whose action lost the race that the list shows the current state", () => {
    assert.match(
      problemOf({ kind: "refused", status: 409, code: "state_conflict" }),
      /list shows where it is now/,
    );
  });

  test("never calls an unanswered request done", () => {
    const message = problemOf({ kind: "unconfirmed", reason: "Failed to fetch" });
    assert.match(message, /not confirm/);
    assert.match(message, /Failed to fetch/);
  });
});
