import assert from "node:assert/strict";
import { test } from "node:test";

import { authPrefixes, generateAuthId } from "../src/ids.ts";

const typeIdFormat = /^[a-z]{3,5}_[0-7][0-9a-hjkmnp-tv-z]{25}$/;

test("every listed model gets its own prefix", () => {
  const prefixes = Object.values(authPrefixes);
  assert.equal(new Set(prefixes).size, prefixes.length);
  for (const [model, prefix] of Object.entries(authPrefixes)) {
    const id = generateAuthId({ model });
    assert.match(id, typeIdFormat);
    assert.ok(id.startsWith(`${prefix}_`), id);
    assert.ok(id.length <= 32, id);
  }
});

test("a model with no listed prefix fails loudly", () => {
  assert.throws(
    () => generateAuthId({ model: "team" }),
    /No id prefix is listed for Better Auth model "team"/,
  );
});

test("ids sort by creation time", async () => {
  const first = generateAuthId({ model: "user" });
  await new Promise((resolve) => setTimeout(resolve, 2));
  const second = generateAuthId({ model: "user" });
  assert.ok(first < second, `${first} < ${second}`);
});
