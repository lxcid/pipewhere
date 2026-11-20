"use client";

import { useState, type FormEvent } from "react";

import { useAuth } from "./auth-client";
import { ErrorText, useAction } from "./ui";

export function SignIn({ note }: { note?: string }) {
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
      {note && <p className="muted">{note}</p>}
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
