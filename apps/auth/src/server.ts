import { serve } from "@hono/node-server";
import pg from "pg";

import { createApp } from "./app.ts";
import { createAuth } from "./auth.ts";
import { readEnv } from "./env.ts";

const env = readEnv();
const pool = new pg.Pool({ connectionString: env.databaseUrl });
const app = createApp(createAuth(pool, env), env);
const port = Number(process.env.PORT ?? 3001);

serve({ fetch: app.fetch, port }, () => {
  console.log(`auth listening on ${port}`);
});
