"use client";

import { useParams } from "next/navigation";
import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";

import { useAuth } from "../../auth-client";
import { Channels } from "../../channels";
import { OrganizationCard, type Organization } from "../../members";
import { Publications } from "../../publications";
import { isOwner, manages } from "../../roles";
import { Claims, ErrorText, Shell, useAction } from "../../ui";

interface ApiKey {
  id: string;
  name: string | null;
  start: string | null;
  createdAt: string | Date;
}

export default function OrganizationPage() {
  const { organizationId } = useParams<{ organizationId: string }>();
  const { client } = useAuth();
  const session = client.useSession();
  const [organization, setOrganization] = useState<Organization | null>(null);
  const [missing, setMissing] = useState<string | null>(null);

  const shown = useRef(organizationId);
  useEffect(() => {
    shown.current = organizationId;
  }, [organizationId]);

  // An answer for an organization the page has moved on from is dropped, and
  // the page shows only the organization its address names.
  const load = useCallback(async () => {
    const { data } = await client.organization.getFullOrganization({ query: { organizationId } });
    if (shown.current !== organizationId) return;
    if (data) setOrganization(data as unknown as Organization);
    else setMissing(organizationId);
  }, [client, organizationId]);

  // Better Auth refetches the session when the window regains focus, so a role
  // changed elsewhere shows on return.
  useEffect(() => {
    if (session.data) load();
  }, [load, session.data]);

  if (session.isPending) {
    return (
      <Shell>
        <p className="muted">Loading…</p>
      </Shell>
    );
  }
  if (!session.data) {
    return (
      <Shell>
        <p>
          <a href="/">Sign in</a> to see this organization.
        </p>
      </Shell>
    );
  }
  if (missing === organizationId) {
    return (
      <Shell>
        <p>This organization does not exist, or you are not a member.</p>
      </Shell>
    );
  }
  const userId = session.data.user.id;
  const me = organization?.members.find((member) => member.userId === userId);
  if (organization?.id !== organizationId || !me) {
    return (
      <Shell>
        <p className="muted">Loading…</p>
      </Shell>
    );
  }

  return (
    <Shell>
      <OrganizationCard organization={organization} me={me} reload={load} />
      <Channels organizationId={organization.id} canConnect={manages(me.role)} />
      <Publications key={organization.id} organizationId={organization.id} />
      {isOwner(me.role) ? (
        <ApiKeys organizationId={organization.id} />
      ) : (
        <section className="card">
          <h2>API keys</h2>
          <p className="muted">Only an owner sees and manages the organization&apos;s API keys.</p>
        </section>
      )}
    </Shell>
  );
}

function ApiKeys({ organizationId }: { organizationId: string }) {
  const { client, authUrl } = useAuth();
  const [keys, setKeys] = useState<ApiKey[] | null>(null);
  const [created, setCreated] = useState<{ id: string; key: string } | null>(null);
  const [token, setToken] = useState<string | null>(null);
  const [name, setName] = useState("");
  const { pending, error, run } = useAction();

  const load = useCallback(async () => {
    const result = await client.apiKey.list({ query: { organizationId } });
    if (result.data) setKeys(result.data.apiKeys as ApiKey[]);
    return result;
  }, [client, organizationId]);

  useEffect(() => {
    run(load);
  }, [load]);

  function create(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    run(async () => {
      const result = await client.apiKey.create({ organizationId, name });
      if (result.error) return result;
      setCreated({ id: result.data.id, key: result.data.key });
      setToken(null);
      setName("");
      return load();
    });
  }

  function revoke(keyId: string) {
    run(async () => {
      const result = await client.apiKey.delete({ keyId });
      if (result.error) return result;
      if (created?.id === keyId) setCreated(null);
      return load();
    });
  }

  function exchange(key: string) {
    run(async () => {
      const response = await fetch(`${authUrl}/api-key/token`, {
        method: "POST",
        headers: { "x-api-key": key },
      });
      if (!response.ok) throw new Error(`Auth refused the key (${response.status}).`);
      setToken(((await response.json()) as { token: string }).token);
    });
  }

  return (
    <section className="card">
      <h2>API keys</h2>
      <p className="muted">
        A key belongs to the organization, not to whoever created it. It keeps working after its
        creator leaves.
      </p>
      {keys?.length ? (
        <ul className="list">
          {keys.map((key) => (
            <li key={key.id}>
              <span>
                {key.name ?? "Unnamed"} <span className="muted">{key.start}…</span>
              </span>
              <code>{key.id}</code>
              <button className="ghost" onClick={() => revoke(key.id)} disabled={pending}>
                Revoke
              </button>
            </li>
          ))}
        </ul>
      ) : (
        keys && <p>No keys yet.</p>
      )}
      <form onSubmit={create} className="row">
        <input
          placeholder="Key name"
          value={name}
          required
          onChange={(event) => setName(event.target.value)}
        />
        <button type="submit" disabled={pending}>
          Create key
        </button>
      </form>
      <ErrorText error={error} />
      {created && (
        <div className="notice">
          <p>Copy this key now. It is not shown again.</p>
          <code className="secret">{created.key}</code>
          <button onClick={() => exchange(created.key)} disabled={pending}>
            Exchange for an access token
          </button>
        </div>
      )}
      {token && <Claims token={token} />}
    </section>
  );
}
