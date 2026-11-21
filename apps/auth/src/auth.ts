import { apiKey } from "@better-auth/api-key";
import { betterAuth, type BetterAuthOptions } from "better-auth";
import { APIError } from "better-auth/api";
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
        // Auth sends no email, so the invitation's link admits the invited
        // address (00012 D1). Set explicitly because Better Auth's default
        // depends on the id generator.
        requireEmailVerificationOnInvitation: false,
        organizationHooks: {
          // Any member can read an invitation's id, and anyone can sign up as
          // an address without an account, so an invitation must grant no
          // more than a member holds. A promotion follows the join (00012 D1).
          beforeCreateInvitation: async ({ invitation }) => {
            if (invitation.role !== "member") {
              throw new APIError("BAD_REQUEST", {
                message: "An invitation is for the member role. Change the role after they join.",
              });
            }
          },
        },
      }),
      apiKey({
        references: "organization",
        // The default allows 10 verifications a day per key. A client that
        // exchanges its key every 15 minutes needs 96.
        rateLimit: { enabled: false },
      }),
      jwt({
        // Web asks for a token when it needs one, so get-session need not sign one each time.
        disableSettingJwtHeader: true,
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
