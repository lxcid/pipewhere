import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { createLocalJWKSet, jwtVerify, type JSONWebKeySet } from "jose";

import { accessTokenAudience } from "../src/auth.ts";
import { json, signUp, startInstance, type Caller, type TestInstance } from "./harness.ts";

let instance: TestInstance;
let owner: Caller;
let organizationId: string;

before(async () => {
  instance = await startInstance();
  owner = await signUp(instance, "owner@example.com");
  const organization = await json<{ id: string }>(
    await owner.request("/api/auth/organization/create", {
      method: "POST",
      body: JSON.stringify({ name: "Personal", slug: "personal" }),
    }),
  );
  organizationId = organization.id;
});

after(() => instance.close());

async function verify(token: string) {
  const jwks = await json<JSONWebKeySet>(await instance.app.request("/api/auth/jwks"));
  const { payload } = await jwtVerify(token, createLocalJWKSet(jwks), {
    issuer: instance.env.authUrl,
    audience: accessTokenAudience,
  });
  return payload;
}

async function createKey(caller: Caller, name = "ci") {
  return json<{ id: string; key: string; referenceId: string }>(
    await caller.request("/api/auth/api-key/create", {
      method: "POST",
      body: JSON.stringify({ organizationId, name }),
    }),
  );
}

function exchangeKey(key: string | undefined) {
  return instance.app.request("/api-key/token", {
    method: "POST",
    headers: key === undefined ? {} : { "x-api-key": key },
  });
}

test("a session exchanges for a token carrying only the user's id and standard claims", async () => {
  const { token } = await json<{ token: string }>(await owner.request("/api/auth/token"));
  const claims = await verify(token);
  assert.equal(claims.sub, owner.userId);
  assert.match(owner.userId, /^usr_/);
  assert.deepEqual(Object.keys(claims).sort(), ["aud", "exp", "iat", "iss", "sub"]);
  assert.ok(claims.exp! - claims.iat! <= 15 * 60);
});

test("an organization's API key exchanges for a token carrying the key's id", async () => {
  const created = await createKey(owner);
  assert.equal(created.referenceId, organizationId);
  assert.match(created.id, /^key_/);

  const { token } = await json<{ token: string }>(await exchangeKey(created.key));
  const claims = await verify(token);
  assert.equal(claims.sub, created.id);
  assert.deepEqual(Object.keys(claims).sort(), ["aud", "exp", "iat", "iss", "sub"]);
  assert.ok(claims.exp! - claims.iat! <= 15 * 60);
});

test("a key exchanges more often than Better Auth's default daily limit", async () => {
  const created = await createKey(owner, "frequent");
  for (let i = 0; i < 12; i++) {
    assert.equal((await exchangeKey(created.key)).status, 200, `exchange ${i + 1}`);
  }
});

test("a missing, unknown, disabled, or expired key is refused", async () => {
  assert.equal((await exchangeKey(undefined)).status, 401);
  assert.equal((await exchangeKey("not-a-key")).status, 401);

  const disabled = await createKey(owner, "disabled");
  await json(
    await owner.request("/api/auth/api-key/update", {
      method: "POST",
      body: JSON.stringify({ keyId: disabled.id, enabled: false }),
    }),
  );
  assert.equal((await exchangeKey(disabled.key)).status, 401);

  const expired = await createKey(owner, "expired");
  await instance.authPool.query(
    `UPDATE apikey SET "expiresAt" = now() - interval '1 minute' WHERE id = $1`,
    [expired.id],
  );
  assert.equal((await exchangeKey(expired.key)).status, 401);
});

test("a key keeps working after the member who created it is removed", async () => {
  // Better Auth's default roles let only an owner create an organization's keys.
  const coOwner = await signUp(instance, "co-owner@example.com");
  await instance.authPool.query(`UPDATE "user" SET "emailVerified" = true WHERE id = $1`, [
    coOwner.userId,
  ]);
  const invitation = await json<{ id: string }>(
    await owner.request("/api/auth/organization/invite-member", {
      method: "POST",
      body: JSON.stringify({ organizationId, email: "co-owner@example.com", role: "owner" }),
    }),
  );
  await json(
    await coOwner.request("/api/auth/organization/accept-invitation", {
      method: "POST",
      body: JSON.stringify({ invitationId: invitation.id }),
    }),
  );

  const created = await createKey(coOwner, "made-by-co-owner");
  await json(
    await owner.request("/api/auth/organization/remove-member", {
      method: "POST",
      body: JSON.stringify({ organizationId, memberIdOrEmail: "co-owner@example.com" }),
    }),
  );

  const { token } = await json<{ token: string }>(await exchangeKey(created.key));
  assert.equal((await verify(token)).sub, created.id);
});
