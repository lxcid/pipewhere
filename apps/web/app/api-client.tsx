"use client";

import { createContext, useCallback, useContext, type ReactNode } from "react";

import { useAuth } from "./auth-client";

const ApiContext = createContext<string | null>(null);

export function ApiProvider({ apiUrl, children }: { apiUrl: string; children: ReactNode }) {
  return <ApiContext value={apiUrl}>{children}</ApiContext>;
}

/** API refused a request. `code` is the error body's code. */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
  ) {
    super(`API refused the request: ${code} (${status}).`);
  }
}

/** Calls API as the signed-in user, with a fresh token from Auth. */
export function useApi() {
  const apiUrl = useContext(ApiContext);
  if (!apiUrl) throw new Error("useApi must be used inside ApiProvider");
  const { authUrl } = useAuth();
  return useCallback(
    async <T,>(path: string, request: { method?: string; body?: unknown } = {}): Promise<T> => {
      const issued = await fetch(`${authUrl}/api/auth/token`, { credentials: "include" });
      if (!issued.ok) throw new Error(`Auth did not issue a token (${issued.status}).`);
      const { token } = (await issued.json()) as { token: string };
      const headers: Record<string, string> = { authorization: `Bearer ${token}` };
      if (request.body !== undefined) headers["content-type"] = "application/json";
      const response = await fetch(`${apiUrl}${path}`, {
        method: request.method ?? "GET",
        headers,
        body: request.body === undefined ? undefined : JSON.stringify(request.body),
      });
      const body = await response.json().catch(() => null);
      if (!response.ok) throw new ApiError(response.status, body?.code ?? "unknown");
      return body as T;
    },
    [apiUrl, authUrl],
  );
}
