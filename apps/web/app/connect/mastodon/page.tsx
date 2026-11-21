"use client";

import { useRouter } from "next/navigation";
import { useEffect, useRef, useState } from "react";

import { ApiError, useApi } from "../../api-client";
import { useAuth } from "../../auth-client";
import { pendingConnection } from "../../channels";
import { Shell } from "../../ui";

const REFUSED: Record<string, string> = {
  authorization_not_found:
    "This connection was already finished, or it is over an hour old. Start again from your organization's page.",
  authorization_refused: "The instance refused the approval. Start again from your organization's page.",
  instance_unavailable: "The instance did not answer. Try again from your organization's page.",
};

/** Where a Mastodon instance sends the browser after the operator approves Pipewhere. */
export default function ConnectMastodon() {
  const { client } = useAuth();
  const session = client.useSession();
  const api = useApi();
  const router = useRouter();
  const [problem, setProblem] = useState<string | null>(null);
  // A code works once, and React runs an effect twice in development.
  const started = useRef(false);

  useEffect(() => {
    if (!session.data || started.current) return;
    started.current = true;
    const query = new URLSearchParams(window.location.search);
    const state = query.get("state");
    const code = query.get("code");
    if (query.get("error")) {
      setProblem("Pipewhere was not approved at the instance, so nothing was connected.");
      return;
    }
    const organizationId = state && sessionStorage.getItem(pendingConnection(state));
    if (!state || !code || !organizationId) {
      setProblem("Start connecting an account from your organization's page, in this browser tab.");
      return;
    }
    api(`/v1/orgs/${organizationId}/channels/mastodon`, {
      method: "POST",
      body: { authorization_id: state, code },
    })
      .then(() => {
        sessionStorage.removeItem(pendingConnection(state));
        router.replace(`/orgs/${organizationId}`);
      })
      .catch((caught) => {
        const known = caught instanceof ApiError ? REFUSED[caught.code] : undefined;
        setProblem(known ?? (caught instanceof Error ? caught.message : String(caught)));
      });
  }, [api, router, session.data]);

  return (
    <Shell>
      {session.isPending ? (
        <p className="muted">Loading…</p>
      ) : !session.data ? (
        <p>
          <a href="/">Sign in</a>, then start connecting the account again from your
          organization's page.
        </p>
      ) : problem ? (
        <p className="error">{problem}</p>
      ) : (
        <p className="muted">Connecting the account…</p>
      )}
    </Shell>
  );
}
