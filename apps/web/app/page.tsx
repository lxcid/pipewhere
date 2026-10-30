"use client";

import Link from "next/link";
import { useEffect, useState, type FormEvent } from "react";

import { useAuth } from "./auth-client";
import { Claims, ErrorText, Shell, useAction } from "./ui";

export default function Home() {
  const { client } = useAuth();
  const session = client.useSession();
  // While signed out, Better Auth marks every refetch as pending, such as the
  // one on window focus. Waiting only for the first answer keeps the sign-in
  // form mounted, with what the user typed.
  const [answered, setAnswered] = useState(false);
  useEffect(() => {
    if (!session.isPending) setAnswered(true);
  }, [session.isPending]);
  if (!answered) {
    return (
      <Shell>
        <p className="muted">Loading…</p>
      </Shell>
    );
  }
  return <Shell>{session.data ? <Dashboard /> : <SignIn />}</Shell>;
}

function SignIn() {
  const { client } = useAuth();
  const [mode, setMode] = useState<"sign-in" | "sign-up">("sign-in");
  const { pending, error, run } = useAction();

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const email = String(form.get("email"));
    const password = String(form.get("password"));
    run(() =>
      mode === "sign-in"
        ? client.signIn.email({ email, password })
        : client.signUp.email({ email, password, name: String(form.get("name")) }),
    );
  }

  return (
    <section className="card narrow">
      <h1>{mode === "sign-in" ? "Sign in" : "Create an account"}</h1>
      <form onSubmit={submit} className="stack">
        {mode === "sign-up" && (
          <label>
            Name
            <input name="name" required autoComplete="name" />
          </label>
        )}
        <label>
          Email
          <input name="email" type="email" required autoComplete="email" />
        </label>
        <label>
          Password
          <input
            name="password"
            type="password"
            required
            minLength={8}
            autoComplete={mode === "sign-in" ? "current-password" : "new-password"}
          />
        </label>
        <ErrorText error={error} />
        <button type="submit" disabled={pending}>
          {mode === "sign-in" ? "Sign in" : "Create account"}
        </button>
      </form>
      <p className="muted">
        {mode === "sign-in" ? "No account yet? " : "Already have an account? "}
        <button
          className="link"
          onClick={() => setMode(mode === "sign-in" ? "sign-up" : "sign-in")}
        >
          {mode === "sign-in" ? "Create one" : "Sign in"}
        </button>
      </p>
    </section>
  );
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
