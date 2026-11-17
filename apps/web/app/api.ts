/** API refused a request. `code` is the error body's code. */
export class ApiError extends Error {
  readonly status: number;
  readonly code: string;

  constructor(status: number, code: string) {
    super(`API refused the request: ${code} (${status}).`);
    this.status = status;
    this.code = code;
  }
}

export interface ApiRequest {
  method?: string;
  body?: unknown;
  headers?: Record<string, string>;
}

/**
 * Calls API as the signed-in user, with a fresh token from Auth. A successful
 * status whose body is not a JSON object throws a plain error, because the
 * request may still have taken effect.
 */
export async function callApi<T>(
  authUrl: string,
  apiUrl: string,
  path: string,
  request: ApiRequest = {},
): Promise<T> {
  const issued = await fetch(`${authUrl}/api/auth/token`, { credentials: "include" });
  if (!issued.ok) throw new Error(`Auth did not issue a token (${issued.status}).`);
  const { token } = (await issued.json()) as { token: string };
  const headers: Record<string, string> = {
    ...request.headers,
    authorization: `Bearer ${token}`,
  };
  if (request.body !== undefined) headers["content-type"] = "application/json";
  const response = await fetch(`${apiUrl}${path}`, {
    method: request.method ?? "GET",
    headers,
    body: request.body === undefined ? undefined : JSON.stringify(request.body),
  });
  const body: unknown = await response.json().catch(() => null);
  const read = typeof body === "object" && body !== null ? (body as Record<string, unknown>) : null;
  if (!response.ok) {
    throw new ApiError(response.status, typeof read?.code === "string" ? read.code : "unknown");
  }
  if (!read) throw new Error(`API's answer could not be read (${response.status}).`);
  return read as T;
}

/**
 * How API answered a request. `unconfirmed` means no answer says whether it
 * took effect: the connection failed, the answer could not be read, or API
 * answered 5xx.
 */
export type Answer<T> =
  | { kind: "answered"; value: T }
  | { kind: "refused"; status: number; code: string }
  | { kind: "unconfirmed"; reason: string };

export async function answer<T>(call: () => Promise<T>): Promise<Answer<T>> {
  try {
    return { kind: "answered", value: await call() };
  } catch (caught) {
    if (caught instanceof ApiError && caught.status < 500) {
      return { kind: "refused", status: caught.status, code: caught.code };
    }
    return {
      kind: "unconfirmed",
      reason: caught instanceof Error ? caught.message : String(caught),
    };
  }
}
