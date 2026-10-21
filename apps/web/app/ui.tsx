"use client";

import Link from "next/link";
import { useState, type ReactNode } from "react";

import { useAuth } from "./auth-client";

export function Shell({ children }: { children: ReactNode }) {
  const { client } = useAuth();
  const session = client.useSession();
  return (
    <>
      <header className="topbar">
        <Link href="/" className="brand">
          Pipewhere
        </Link>
        {session.data && (
          <div className="who">
            <span className="muted">{session.data.user.email}</span>
            <button className="ghost" onClick={() => client.signOut()}>
              Sign out
            </button>
          </div>
        )}
      </header>
      <main>{children}</main>
    </>
  );
}

export function decodeClaims(token: string): Record<string, unknown> {
  const payload = token.split(".")[1].replace(/-/g, "+").replace(/_/g, "/");
  return JSON.parse(atob(payload));
}

export function Claims({ token }: { token: string }) {
  const claims = decodeClaims(token);
  const expires =
    typeof claims.exp === "number" ? new Date(claims.exp * 1000).toLocaleTimeString() : null;
  return (
    <div className="claims">
      <pre>{JSON.stringify(claims, null, 2)}</pre>
      {expires && <p className="muted">Expires at {expires}.</p>}
    </div>
  );
}

export function ErrorText({ error }: { error: string | null }) {
  return error ? <p className="error">{error}</p> : null;
}

export function useAction() {
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function run(action: () => Promise<{ error?: { message?: string } | null } | void>) {
    setPending(true);
    setError(null);
    try {
      const result = await action();
      if (result && result.error) setError(result.error.message ?? "Something went wrong.");
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : String(caught));
    } finally {
      setPending(false);
    }
  }
  return { pending, error, run };
}
