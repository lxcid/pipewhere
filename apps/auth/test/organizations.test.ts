import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { json, signUp, startInstance, type Caller, type TestInstance } from "./harness.ts";

let instance: TestInstance;
let owner: Caller;

before(async () => {
  instance = await startInstance();
  owner = await signUp(instance, "owner@example.com");
});

after(() => instance.close());

async function createOrganization(caller: Caller, slug: string) {
  return json<{ id: string }>(
    await caller.request("/api/auth/organization/create", {
      method: "POST",
      body: JSON.stringify({ name: slug, slug }),
    }),
  );
}

async function activeOrganizations(userId: string) {
  const { rows } = await instance.authPool.query<{ activeOrganizationId: string | null }>(
    `SELECT "activeOrganizationId" FROM session WHERE "userId" = $1`,
    [userId],
  );
  return rows.map((row) => row.activeOrganizationId);
}

async function invite(organizationId: string, email: string) {
  return json<{ id: string }>(
    await owner.request("/api/auth/organization/invite-member", {
      method: "POST",
      body: JSON.stringify({ organizationId, email, role: "member" }),
    }),
  );
}

test("one user holds a personal and a company organization at once", async () => {
  const personal = await createOrganization(owner, "personal");
  const company = await createOrganization(owner, "company");
  assert.match(personal.id, /^org_/);

  const list = await json<{ id: string }[]>(await owner.request("/api/auth/organization/list"));
  assert.deepEqual(
    list.map((organization) => organization.id).sort(),
    [personal.id, company.id].sort(),
  );

  for (const organization of [personal, company]) {
    const full = await json<{ id: string }>(
      await owner.request(
        `/api/auth/organization/get-full-organization?organizationId=${organization.id}`,
      ),
    );
    assert.equal(full.id, organization.id);
  }
});

test("creating an organization leaves no active organization, so a call naming none acts on neither", async () => {
  await createOrganization(owner, "no-active");
  assert.deepEqual(await activeOrganizations(owner.userId), [null]);

  const unnamed = await owner.request("/api/auth/organization/get-full-organization");
  assert.equal(await unnamed.text(), "null");

  const invitation = await owner.request("/api/auth/organization/invite-member", {
    method: "POST",
    body: JSON.stringify({ email: "nobody@example.com", role: "member" }),
  });
  assert.equal(invitation.status, 400);
});

test("setting an active organization explicitly is overridden", async () => {
  const organization = await createOrganization(owner, "explicit");
  await owner.request("/api/auth/organization/set-active", {
    method: "POST",
    body: JSON.stringify({ organizationId: organization.id }),
  });
  assert.deepEqual(await activeOrganizations(owner.userId), [null]);
});

test("an invitee with an unverified email cannot join; once verified, joining leaves no active organization", async () => {
  const organization = await createOrganization(owner, "team");
  const invitee = await signUp(instance, "invitee@example.com");
  const invitation = await invite(organization.id, "invitee@example.com");
  assert.match(invitation.id, /^inv_/);

  const refused = await invitee.request("/api/auth/organization/accept-invitation", {
    method: "POST",
    body: JSON.stringify({ invitationId: invitation.id }),
  });
  assert.equal(refused.status, 403);

  await instance.authPool.query(`UPDATE "user" SET "emailVerified" = true WHERE id = $1`, [
    invitee.userId,
  ]);
  const accepted = await json<{ member: { id: string } }>(
    await invitee.request("/api/auth/organization/accept-invitation", {
      method: "POST",
      body: JSON.stringify({ invitationId: invitation.id }),
    }),
  );
  assert.match(accepted.member.id, /^mem_/);
  assert.deepEqual(await activeOrganizations(invitee.userId), [null]);
});

test("deleting an organization is refused", async () => {
  const organization = await createOrganization(owner, "kept");
  const response = await owner.request("/api/auth/organization/delete", {
    method: "POST",
    body: JSON.stringify({ organizationId: organization.id }),
  });
  assert.equal(response.status, 404);
  assert.match(await response.text(), /ORGANIZATION_DELETION_DISABLED/);
  const { rowCount } = await instance.authPool.query(`SELECT 1 FROM organization WHERE id = $1`, [
    organization.id,
  ]);
  assert.equal(rowCount, 1);
});

test("every row Auth writes carries its table's prefix", async () => {
  await owner.request("/api/auth/token");
  const expected: Record<string, string> = {
    user: "usr_",
    session: "ses_",
    account: "acc_",
    organization: "org_",
    member: "mem_",
    invitation: "inv_",
    jwks: "jwk_",
  };
  for (const [table, prefix] of Object.entries(expected)) {
    const { rows } = await instance.authPool.query<{ id: string }>(`SELECT id FROM "${table}"`);
    assert.ok(rows.length > 0, `${table} has rows`);
    for (const { id } of rows) assert.ok(id.startsWith(prefix), `${table}: ${id}`);
  }
});
