import { typeid } from "typeid-js";

// Keyed by Better Auth's model name, which is what its id generator receives.
export const authPrefixes: Readonly<Record<string, string>> = {
  user: "usr",
  session: "ses",
  account: "acc",
  verification: "ver",
  organization: "org",
  member: "mem",
  invitation: "inv",
  apikey: "key",
  jwks: "jwk",
};

export function generateAuthId({ model }: { model: string }): string {
  const prefix = authPrefixes[model];
  if (prefix === undefined) {
    throw new Error(
      `No id prefix is listed for Better Auth model "${model}". Add one to authPrefixes in apps/auth/src/ids.ts.`,
    );
  }
  return typeid(prefix).toString();
}
