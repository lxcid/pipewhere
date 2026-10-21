import { execFileSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { fileURLToPath } from "node:url";

import pg from "pg";

import { createApp } from "../src/app.ts";
import { authOptions, createAuth } from "../src/auth.ts";
import type { AuthEnv } from "../src/env.ts";
import { migrate } from "../src/migrate.ts";

// A superuser on a disposable server. `pnpm test:db` starts one.
const adminUrl =
  process.env.TEST_DATABASE_URL ?? "postgres://postgres:test@127.0.0.1:5433/postgres";
const setupSql = fileURLToPath(new URL("../../../infra/postgres/setup.sql", import.meta.url));
const authPassword = "auth-test";
const apiPassword = "api-test";
export const webUrl = "http://localhost:3000";

export interface TestInstance {
  env: AuthEnv;
  auth: ReturnType<typeof createAuth>;
  app: ReturnType<typeof createApp>;
  authPool: pg.Pool;
  apiPool: pg.Pool;
  adminPool: pg.Pool;
  close(): Promise<void>;
}

function urlFor(database: string, user?: string, password?: string): string {
  const url = new URL(adminUrl);
  url.pathname = `/${database}`;
  if (user) url.username = user;
  if (password) url.password = password;
  return url.toString();
}

export async function startInstance(): Promise<TestInstance> {
  const database = `pipewhere_test_${randomBytes(4).toString("hex")}`;
  const server = new pg.Pool({ connectionString: adminUrl, max: 1 });
  await server.query(`CREATE DATABASE ${database}`);
  await server.end();

  execFileSync(
    "psql",
    [
      "-q",
      "-v",
      `auth_password=${authPassword}`,
      "-v",
      `api_password=${apiPassword}`,
      "-f",
      setupSql,
      urlFor(database),
    ],
    { stdio: ["ignore", "ignore", "inherit"] },
  );

  const env: AuthEnv = {
    databaseUrl: urlFor(database, "pipewhere_auth", authPassword),
    secret: randomBytes(32).toString("base64"),
    authUrl: "http://localhost:3001",
    webUrl,
  };
  const authPool = new pg.Pool({ connectionString: env.databaseUrl });
  const apiPool = new pg.Pool({ connectionString: urlFor(database, "pipewhere_api", apiPassword) });
  const adminPool = new pg.Pool({ connectionString: urlFor(database) });
  await migrate(authPool, authOptions(authPool, env));
  const auth = createAuth(authPool, env);

  return {
    env,
    auth,
    app: createApp(auth, env),
    authPool,
    apiPool,
    adminPool,
    async close() {
      await Promise.all([authPool.end(), apiPool.end(), adminPool.end()]);
      const cleanup = new pg.Pool({ connectionString: adminUrl, max: 1 });
      await cleanup.query(`DROP DATABASE ${database} WITH (FORCE)`);
      await cleanup.end();
    },
  };
}

export interface Caller {
  cookie: string;
  userId: string;
  request(path: string, init?: RequestInit): Promise<Response>;
}

// Sends requests the way Web's pages do: from Web's origin, with the session cookie.
export async function signUp(instance: TestInstance, email: string): Promise<Caller> {
  const response = await instance.app.request("/api/auth/sign-up/email", {
    method: "POST",
    headers: { "content-type": "application/json", origin: webUrl },
    body: JSON.stringify({ email, password: "correct horse battery", name: email.split("@")[0] }),
  });
  if (!response.ok) throw new Error(`sign-up failed: ${response.status} ${await response.text()}`);
  const body = (await response.json()) as { user: { id: string } };
  const cookie = response.headers
    .getSetCookie()
    .map((header) => header.split(";")[0])
    .join("; ");
  return {
    cookie,
    userId: body.user.id,
    async request(path, init = {}) {
      const headers = new Headers(init.headers);
      headers.set("cookie", cookie);
      headers.set("origin", webUrl);
      if (init.body && !headers.has("content-type"))
        headers.set("content-type", "application/json");
      return instance.app.request(path, { ...init, headers });
    },
  };
}

export async function json<T>(response: Response): Promise<T> {
  const text = await response.text();
  if (!response.ok) throw new Error(`${response.status}: ${text}`);
  return JSON.parse(text) as T;
}
