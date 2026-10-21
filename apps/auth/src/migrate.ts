import { getMigrations } from "better-auth/db/migration";
import pg from "pg";

import { authOptions } from "./auth.ts";
import { readEnv } from "./env.ts";

// API reads only these columns, per 00001 D2. A column Better Auth adds stays
// hidden from API until it is listed here.
export const apiColumnGrants: Readonly<Record<string, readonly string[]>> = {
  organization: ["id", "name", "slug"],
  member: ["id", "organizationId", "userId", "role"],
  apikey: ["id", "referenceId", "permissions", "enabled", "expiresAt"],
};

export const apiRole = "pipewhere_api";

// Takes Better Auth's options rather than an instance: constructing one checks
// the schema, and logs an error for every table this is about to create.
export async function migrate(
  pool: pg.Pool,
  options: ReturnType<typeof authOptions>,
): Promise<void> {
  const { runMigrations } = await getMigrations(options);
  await runMigrations();

  const client = await pool.connect();
  try {
    await client.query("BEGIN");
    await client.query(`GRANT USAGE ON SCHEMA auth TO ${apiRole}`);
    for (const [table, columns] of Object.entries(apiColumnGrants)) {
      const list = columns.map((column) => `"${column}"`).join(", ");
      await client.query(`GRANT SELECT (${list}) ON auth."${table}" TO ${apiRole}`);
    }
    await client.query(`GRANT REFERENCES ("id") ON auth."organization" TO ${apiRole}`);
    await client.query("COMMIT");
  } catch (error) {
    await client.query("ROLLBACK");
    throw error;
  } finally {
    client.release();
  }
}

if (import.meta.main) {
  const env = readEnv();
  const pool = new pg.Pool({ connectionString: env.databaseUrl });
  await migrate(pool, authOptions(pool, env));
  await pool.end();
  console.log("auth migrations applied");
}
