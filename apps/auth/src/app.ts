import { Hono } from "hono";
import { cors } from "hono/cors";

import type { Auth } from "./auth.ts";
import type { AuthEnv } from "./env.ts";

export function createApp(auth: Auth, env: AuthEnv) {
  const app = new Hono();

  app.use(
    "*",
    cors({
      origin: env.webUrl,
      credentials: true,
      allowHeaders: ["content-type", "x-api-key"],
      allowMethods: ["GET", "POST", "OPTIONS"],
    }),
  );

  app.get("/health", (c) => c.json({ ok: true }));

  // Better Auth turns an API key into a session only for keys a user owns.
  // Pipewhere's keys belong to organizations, so the exchange is ours.
  app.post("/api-key/token", async (c) => {
    const key = c.req.header("x-api-key");
    if (!key) return c.json({ code: "API_KEY_MISSING" }, 401);
    const result = await auth.api.verifyApiKey({ body: { key } });
    if (!result.valid || !result.key) {
      return c.json({ code: result.error?.code ?? "INVALID_API_KEY" }, 401);
    }
    const { token } = await auth.api.signJWT({
      body: { payload: { sub: result.key.id, iat: Math.floor(Date.now() / 1000) } },
    });
    return c.json({ token });
  });

  app.on(["GET", "POST"], "/api/auth/*", (c) => auth.handler(c.req.raw));

  return app;
}
