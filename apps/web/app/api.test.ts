import assert from "node:assert/strict";
import { afterEach, describe, test } from "node:test";

import { answer, callApi } from "./api.ts";
import { Submitter, sendCreate, type Publication } from "./publishing.ts";

const realFetch = globalThis.fetch;
afterEach(() => {
  globalThis.fetch = realFetch;
});

const publication: Publication = {
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
};

/** Auth issues a token; API answers with `respond`. Records API's requests. */
function serve(respond: (request: Request) => Response | Promise<Response>) {
  const requests: Request[] = [];
  globalThis.fetch = async (input, init) => {
    const request = new Request(input, init);
    if (request.url === "http://auth.test/api/auth/token") {
      return Response.json({ token: "token" });
    }
    requests.push(request);
    return respond(request);
  };
  return requests;
}

const api = <T>(path: string, request?: Parameters<typeof callApi>[3]) =>
  callApi<T>("http://auth.test", "http://api.test", path, request);

function truncated(status: number) {
  const body = new ReadableStream({
    start(controller) {
      controller.enqueue(new TextEncoder().encode('{"id":"pub_'));
      controller.error(new TypeError("network error"));
    },
  });
  return new Response(body, { status, headers: { "content-type": "application/json" } });
}

describe("answer", () => {
  test("a successful status whose body cannot be read is unconfirmed, not answered", async () => {
    for (const response of [
      () => new Response("<html>gateway</html>", { status: 201 }),
      () => truncated(201),
      () => new Response("null", { status: 201 }),
    ]) {
      serve(response);
      const sent = await answer(() => api("/v1/orgs/org_01/publications", { method: "POST" }));
      assert.equal(sent.kind, "unconfirmed");
    }
  });

  test("a 5xx or a failed connection is unconfirmed", async () => {
    serve(() => Response.json({ code: "internal" }, { status: 500 }));
    assert.equal((await answer(() => api("/v1/x"))).kind, "unconfirmed");
    serve(() => {
      throw new TypeError("Failed to fetch");
    });
    assert.deepEqual(await answer(() => api("/v1/x")), {
      kind: "unconfirmed",
      reason: "Failed to fetch",
    });
  });

  test("a 4xx is refused with API's code", async () => {
    serve(() => Response.json({ code: "state_conflict" }, { status: 409 }));
    assert.deepEqual(await answer(() => api("/v1/x")), {
      kind: "refused",
      status: 409,
      code: "state_conflict",
    });
  });
});

describe("sendCreate", () => {
  test("sends the key as Idempotency-Key, and the request as JSON", async () => {
    const requests = serve(() => Response.json(publication, { status: 201 }));
    const sent = await sendCreate(api, "org_01")("k".repeat(22), {
      channel_id: "chn_01",
      body: "Hello",
    });
    assert.deepEqual(sent, { kind: "created", publication });
    assert.equal(requests[0].headers.get("idempotency-key"), "k".repeat(22));
    assert.equal(requests[0].headers.get("authorization"), "Bearer token");
    assert.deepEqual(await requests[0].json(), { channel_id: "chn_01", body: "Hello" });
  });

  test("after an unreadable answer to a committed create, sending again reuses the key and finds one publication", async () => {
    // API stores one publication per key, as 00002 D13 says, and answers it again.
    const stored = new Map<string, Publication>();
    let reads = 0;
    const requests = serve((request) => {
      const key = request.headers.get("idempotency-key")!;
      if (!stored.has(key)) stored.set(key, { ...publication, id: `pub_0${stored.size + 1}` });
      reads += 1;
      return reads === 1 ? truncated(201) : Response.json(stored.get(key), { status: 201 });
    });
    const submitter = new Submitter(sendCreate(api, "org_01"));
    const request = { channel_id: "chn_01", body: "Hello" };

    const first = await submitter.submit(request);
    assert.equal(first?.kind, "unconfirmed");
    assert.deepEqual(submitter.unconfirmed, request);

    const second = await submitter.submit(request);
    assert.equal(second?.kind, "created");
    assert.equal(
      requests[1].headers.get("idempotency-key"),
      requests[0].headers.get("idempotency-key"),
    );
    assert.equal(stored.size, 1);
  });
});
