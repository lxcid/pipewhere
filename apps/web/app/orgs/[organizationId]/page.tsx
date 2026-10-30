"use client";

import { useParams } from "next/navigation";
import { useCallback, useEffect, useState, type FormEvent } from "react";

import { useAuth } from "../../auth-client";
import { Claims, ErrorText, Shell, useAction } from "../../ui";

interface Organization {
  id: string;
  name: string;
  slug: string;
  members: { id: string; role: string; user: { name: string; email: string } }[];
}

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
  const [missing, setMissing] = useState(false);

  useEffect(() => {
    if (!session.data) return;
    client.organization.getFullOrganization({ query: { organizationId } }).then(({ data }) => {
      if (data) setOrganization(data as Organization);
      else setMissing(true);
    });
  }, [client, organizationId, session.data]);

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
  if (missing) {
    return (
      <Shell>
        <p>This organization does not exist, or you are not a member.</p>
      </Shell>
    );
  }
  if (!organization) {
    return (
      <Shell>
        <p className="muted">Loading…</p>
      </Shell>
    );
  }

  return (
    <Shell>
      <section className="card">
        <h1>{organization.name}</h1>
        <p className="muted">
          <code>{organization.id}</code> · {organization.slug}
        </p>
        <h2>Members</h2>
        <table>
          <thead>
            <tr>
              <th>Name</th>
              <th>Email</th>
              <th>Role</th>
            </tr>
          </thead>
          <tbody>
            {organization.members.map((member) => (
              <tr key={member.id}>
                <td>{member.user.name}</td>
                <td>{member.user.email}</td>
                <td>{member.role}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>
      <ApiKeys organizationId={organization.id} />
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
