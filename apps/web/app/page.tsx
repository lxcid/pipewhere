"use client";

import Link from "next/link";
import { useState, type FormEvent } from "react";

import { useAuth } from "./auth-client";
import { SignIn } from "./sign-in";
import { Claims, ErrorText, Shell, useAction, useAnsweredSession } from "./ui";

export default function Home() {
  const { session, answered } = useAnsweredSession();
  if (!answered) {
    return (
      <Shell>
        <p className="muted">Loading…</p>
      </Shell>
    );
  }
  return <Shell>{session.data ? <Dashboard /> : <SignIn />}</Shell>;
}

function slugify(name: string) {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");
}

function Dashboard() {
  const { client } = useAuth();
  const organizations = client.useListOrganizations();
  const [name, setName] = useState("");
  const [slug, setSlug] = useState("");
  const [slugEdited, setSlugEdited] = useState(false);
  const { pending, error, run } = useAction();

  function create(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    run(async () => {
      const result = await client.organization.create({ name, slug });
      if (!result.error) {
        setName("");
        setSlug("");
        setSlugEdited(false);
      }
      return result;
    });
  }

  return (
    <>
      <section className="card">
        <h2>Organizations</h2>
        <p className="muted">
          Each organization keeps its own records. Use one for yourself and another for a company.
        </p>
        {organizations.isPending ? (
          <p className="muted">Loading…</p>
        ) : organizations.data?.length ? (
          <ul className="list">
            {organizations.data.map((organization) => (
              <li key={organization.id}>
                <Link href={`/orgs/${organization.id}`}>{organization.name}</Link>
                <code>{organization.id}</code>
              </li>
            ))}
          </ul>
        ) : (
          <p>You have no organizations yet.</p>
        )}
        <form onSubmit={create} className="row">
          <input
            placeholder="Name"
            value={name}
            required
            onChange={(event) => {
              setName(event.target.value);
              if (!slugEdited) setSlug(slugify(event.target.value));
            }}
          />
          <input
            placeholder="slug"
            value={slug}
            required
            pattern="[a-z0-9-]+"
            onChange={(event) => {
              setSlug(event.target.value);
              setSlugEdited(true);
            }}
          />
          <button type="submit" disabled={pending}>
            Create
          </button>
        </form>
        <ErrorText error={error} />
      </section>
      <SessionToken />
    </>
  );
}

function SessionToken() {
  const { authUrl } = useAuth();
  const [token, setToken] = useState<string | null>(null);
  const { pending, error, run } = useAction();

  function fetchToken() {
    run(async () => {
      const response = await fetch(`${authUrl}/api/auth/token`, { credentials: "include" });
      if (!response.ok) throw new Error(`Auth answered ${response.status}.`);
      setToken(((await response.json()) as { token: string }).token);
    });
  }

  return (
    <section className="card">
      <h2>Access token</h2>
      <p className="muted">
        Every client calls the API with the same kind of token. Yours carries only your user id and
        expires within 15 minutes.
      </p>
      <button onClick={fetchToken} disabled={pending}>
        Get a token
      </button>
      <ErrorText error={error} />
      {token && <Claims token={token} />}
    </section>
  );
}
