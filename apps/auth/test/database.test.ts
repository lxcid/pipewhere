import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { getMigrations } from "better-auth/db/migration";

import { apiColumnGrants, migrate } from "../src/migrate.ts";
import { json, signUp, startInstance, type TestInstance } from "./harness.ts";

let instance: TestInstance;
let organizationId: string;

before(async () => {
  instance = await startInstance();
  const owner = await signUp(instance, "owner@example.com");
  const organization = await json<{ id: string }>(
    await owner.request("/api/auth/organization/create", {
      method: "POST",
      body: JSON.stringify({ name: "Personal", slug: "personal" }),
    }),
  );
  organizationId = organization.id;
});

after(() => instance.close());

const permissionDenied = { code: "42501" };

test("API's role reads the granted columns", async () => {
  for (const [table, columns] of Object.entries(apiColumnGrants)) {
    const list = columns.map((column) => `"${column}"`).join(", ");
    await instance.apiPool.query(`SELECT ${list} FROM auth."${table}"`);
  }
});

test("API's role is refused every other column and table in auth", async () => {
  const refused = [
    `SELECT * FROM auth.organization`,
    `SELECT metadata FROM auth.organization`,
    `SELECT key FROM auth.apikey`,
    `SELECT token FROM auth.session`,
    `SELECT id FROM auth."user"`,
    `SELECT password FROM auth.account`,
    `SELECT "privateKey" FROM auth.jwks`,
    `SELECT id FROM auth.invitation`,
  ];
  for (const sql of refused) {
    await assert.rejects(instance.apiPool.query(sql), permissionDenied, sql);
  }
});

test("API's role never writes to auth", async () => {
  await assert.rejects(
    instance.apiPool.query(`UPDATE auth.organization SET name = 'x' WHERE id = $1`, [
      organizationId,
    ]),
    permissionDenied,
  );
  await assert.rejects(instance.apiPool.query(`DELETE FROM auth.member`), permissionDenied);
});

test("a pipewhere table can reference an organization, and the reference blocks its delete", async () => {
  await instance.apiPool.query(
    `CREATE TABLE pipewhere.probe (organization_id text NOT NULL REFERENCES auth.organization (id))`,
  );
  await instance.apiPool.query(`INSERT INTO pipewhere.probe VALUES ($1)`, [organizationId]);
  await assert.rejects(
    instance.authPool.query(`DELETE FROM organization WHERE id = $1`, [organizationId]),
    { code: "23503" },
  );
});

test("Auth's role is refused on the pipewhere schema", async () => {
  await assert.rejects(
    instance.authPool.query(`CREATE TABLE pipewhere.intruder (id text)`),
    permissionDenied,
  );
  await assert.rejects(instance.authPool.query(`SELECT * FROM pipewhere.probe`), permissionDenied);
});

test("migrating again changes nothing", async () => {
  await migrate(instance.authPool, instance.auth.options);
  const { toBeCreated, toBeAdded } = await getMigrations(instance.auth.options);
  assert.deepEqual(toBeCreated, []);
  assert.deepEqual(toBeAdded, []);
});
