"use client";

import { createContext, useCallback, useContext, type ReactNode } from "react";

import { callApi, type ApiRequest } from "./api";
import { useAuth } from "./auth-client";

const ApiContext = createContext<string | null>(null);

export function ApiProvider({ apiUrl, children }: { apiUrl: string; children: ReactNode }) {
  return <ApiContext value={apiUrl}>{children}</ApiContext>;
}

export { ApiError } from "./api";

/** Calls API as the signed-in user, with a fresh token from Auth. */
export function useApi() {
  const apiUrl = useContext(ApiContext);
  if (!apiUrl) throw new Error("useApi must be used inside ApiProvider");
  const { authUrl } = useAuth();
  return useCallback(
    <T,>(path: string, request?: ApiRequest): Promise<T> =>
      callApi<T>(authUrl, apiUrl, path, request),
    [apiUrl, authUrl],
  );
}
