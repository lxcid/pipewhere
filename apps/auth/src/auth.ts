import { apiKey } from "@better-auth/api-key";
import { betterAuth, type BetterAuthOptions } from "better-auth";
import { jwt, organization } from "better-auth/plugins";
import type { Pool } from "pg";

import type { AuthEnv } from "./env.ts";
import { generateAuthId } from "./ids.ts";

export const accessTokenAudience = "pipewhere-api";

export function authOptions(pool: Pool, env: AuthEnv) {
  return {
    database: pool,
    secret: env.secret,
    baseURL: env.authUrl,
    trustedOrigins: [env.webUrl],
    emailAndPassword: { enabled: true },
    telemetry: { enabled: false },
    advanced: { database: { generateId: generateAuthId } },
    databaseHooks: {
      // Better Auth's organization endpoints fall back to the session's active
      // organization when a call names none, and set it themselves on create
      // and join. Keeping it empty makes every call name its organization.
      // User hooks run after plugin hooks, so this write wins.
      session: {
        create: {
          before: async (session) => ({ data: { ...session, activeOrganizationId: null } }),
        },
        update: {
          before: async (session) => ({ data: { ...session, activeOrganizationId: null } }),
        },
      },
    },
    plugins: [
      organization({
        disableOrganizationDeletion: true,
        // A verified email proves the invitee owns the invited address. Set
        // explicitly because Better Auth's default depends on the id generator.
        requireEmailVerificationOnInvitation: true,
      }),
      apiKey({
        references: "organization",
        // The default allows 10 verifications a day per key. A client that
        // exchanges its key every 15 minutes needs 96.
        rateLimit: { enabled: false },
      }),
      jwt({
        jwt: {
          issuer: env.authUrl,
          audience: accessTokenAudience,
          expirationTime: "15m",
          // Without this the plugin copies the whole user record into the claims.
          definePayload: () => ({}),
        },
      }),
    ],
  } satisfies BetterAuthOptions;
}

export function createAuth(pool: Pool, env: AuthEnv) {
  return betterAuth(authOptions(pool, env));
}

export type Auth = ReturnType<typeof createAuth>;
