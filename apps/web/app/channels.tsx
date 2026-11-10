"use client";

import { useCallback, useEffect, useState, type FormEvent } from "react";

import { ApiError, useApi } from "./api-client";
import { ErrorText, useAction } from "./ui";

interface Channel {
  id: string;
  provider: "mastodon" | "threads";
  handle: string | null;
  status: "active" | "disconnected" | "access_revoked" | "account_unavailable";
}

const STATUS: Record<Channel["status"], string> = {
  active: "Can publish",
  disconnected: "Not connected",
  access_revoked: "Access revoked. Connect the account again.",
  account_unavailable: "The instance reports the account unavailable. Check it there.",
};

/** Where the organization is kept while the browser is at the instance. */
export function pendingConnection(authorizationId: string) {
  return `pipewhere:mastodon:${authorizationId}`;
}

export function Channels({ organizationId }: { organizationId: string }) {
  const api = useApi();
  const [channels, setChannels] = useState<Channel[] | null>(null);
  const [instance, setInstance] = useState("");
  const { pending, error, run } = useAction();

  const load = useCallback(async () => {
    const list = await api<{ channels: Channel[] }>(`/v1/orgs/${organizationId}/channels`);
    setChannels(list.channels);
  }, [api, organizationId]);

  useEffect(() => {
    run(load);
  }, [load]);

  function connect(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    run(async () => {
      try {
        const authorization = await api<{ id: string; authorize_url: string }>(
          `/v1/orgs/${organizationId}/channels/mastodon/authorizations`,
          { method: "POST", body: { instance } },
        );
        sessionStorage.setItem(pendingConnection(authorization.id), organizationId);
        window.location.assign(authorization.authorize_url);
      } catch (caught) {
        if (caught instanceof ApiError && caught.code === "invalid_body") {
          throw new Error("Enter the instance's address, such as mastodon.social.");
        }
        if (caught instanceof ApiError && caught.code === "instance_unavailable") {
          throw new Error(`${instance} did not answer as a Mastodon instance does.`);
        }
        throw caught;
      }
    });
  }

  return (
    <section className="card">
      <h2>Channels</h2>
      {channels?.length ? (
        <ul className="list">
          {channels.map((channel) => (
            <li key={channel.id}>
              <span>
                {channel.handle ?? "Never connected"}{" "}
                <span className="muted">{channel.provider}</span>
              </span>
              <span className={channel.status === "active" ? "muted" : "error"}>
                {STATUS[channel.status]}
              </span>
            </li>
          ))}
        </ul>
      ) : (
        channels && <p>No channels yet.</p>
      )}
      <form onSubmit={connect} className="row">
        <input
          placeholder="Mastodon instance, such as mastodon.social"
          value={instance}
          required
          onChange={(event) => setInstance(event.target.value)}
        />
        <button type="submit" disabled={pending}>
          Connect Mastodon
        </button>
      </form>
      <p className="muted">
        Connecting an account the organization already has reconnects its channel.
      </p>
      <ErrorText error={error} />
    </section>
  );
}
