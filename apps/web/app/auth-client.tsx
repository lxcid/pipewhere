"use client";

import { apiKeyClient } from "@better-auth/api-key/client";
import { organizationClient } from "better-auth/client/plugins";
import { createAuthClient } from "better-auth/react";
import { createContext, useContext, useState, type ReactNode } from "react";

function create(authUrl: string) {
  return createAuthClient({
    baseURL: authUrl,
    // Auth is another origin. Its session cookie travels only on credentialed requests.
    fetchOptions: { credentials: "include" },
    plugins: [organizationClient(), apiKeyClient()],
  });
}

type AuthClient = ReturnType<typeof create>;

const AuthContext = createContext<{ client: AuthClient; authUrl: string } | null>(null);

export function AuthProvider({ authUrl, children }: { authUrl: string; children: ReactNode }) {
  const [client] = useState(() => create(authUrl));
  return <AuthContext value={{ client, authUrl }}>{children}</AuthContext>;
}

export function useAuth() {
  const value = useContext(AuthContext);
  if (!value) throw new Error("useAuth must be used inside AuthProvider");
  return value;
}
