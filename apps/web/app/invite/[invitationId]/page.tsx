"use client";

import { useParams, useRouter } from "next/navigation";
import { useEffect, useState } from "react";

import { useAuth } from "../../auth-client";
import { SignIn } from "../../sign-in";
import { ErrorText, Shell, useAction, useAnsweredSession } from "../../ui";

interface Invitation {
  organizationId: string;
  organizationName: string;
  inviterEmail: string;
  email: string;
  role: string;
}

type Lookup =
  | { kind: "found"; invitation: Invitation }
  | { kind: "other-address" }
  | { kind: "gone" }
  | { kind: "inviter-left" }
  | { kind: "failed"; message: string };

export default function InvitationPage() {
  const { invitationId } = useParams<{ invitationId: string }>();
  const { session, answered } = useAnsweredSession();

  if (!answered) {
    return (
      <Shell>
        <p className="muted">Loading…</p>
      </Shell>
    );
  }
  if (!session.data) {
    return (
      <Shell>
        <SignIn note="You were invited to an organization. Sign in, or create an account, with the email address the invitation was sent to." />
      </Shell>
    );
  }
  return (
    <Shell>
      <Answer key={invitationId} invitationId={invitationId} email={session.data.user.email} />
    </Shell>
  );
}

function Answer({ invitationId, email }: { invitationId: string; email: string }) {
  const { client } = useAuth();
  const router = useRouter();
  const [lookup, setLookup] = useState<Lookup | null>(null);
  const [declined, setDeclined] = useState(false);
  const { pending, error, run } = useAction();

  useEffect(() => {
    let current = true;
    client.organization.getInvitation({ query: { id: invitationId } }).then(
      ({ data, error }) => {
        if (!current) return;
        if (data) setLookup({ kind: "found", invitation: data as Invitation });
        // Better Auth answers 403 when the invitation is for another address,
        // and 400 when it was used, cancelled, or has expired, or when its
        // inviter has left the organization.
        else if (error?.status === 403) setLookup({ kind: "other-address" });
        else if (error?.code === "INVITER_IS_NO_LONGER_A_MEMBER_OF_THE_ORGANIZATION") {
          setLookup({ kind: "inviter-left" });
        } else if (error?.status === 400) setLookup({ kind: "gone" });
        else setLookup({ kind: "failed", message: error?.message ?? "Auth did not answer." });
      },
      () => {
        if (current)
          setLookup({
            kind: "failed",
            message: "Pipewhere could not reach Auth. Reload the page.",
          });
      },
    );
    return () => {
      current = false;
    };
  }, [client, invitationId]);

  function join(organizationId: string) {
    run(async () => {
      const result = await client.organization.acceptInvitation({ invitationId });
      if (!result.error) router.push(`/orgs/${organizationId}`);
      return result;
    });
  }

  function decline() {
    run(async () => {
      const result = await client.organization.rejectInvitation({ invitationId });
      if (!result.error) setDeclined(true);
      return result;
    });
  }

  if (!lookup) return <p className="muted">Loading…</p>;
  if (lookup.kind === "other-address") {
    return (
      <section className="card narrow">
        <h1>Invitation</h1>
        <p>
          This invitation is for another email address. You are signed in as {email}. Sign out, then
          sign in with the address it was sent to.
        </p>
        <button onClick={() => client.signOut()}>Sign out</button>
      </section>
    );
  }
  if (lookup.kind === "gone") {
    return (
      <section className="card narrow">
        <h1>Invitation</h1>
        <p>
          This invitation was already answered, was cancelled, or has expired. Ask whoever invited
          you for a new link.
        </p>
      </section>
    );
  }
  if (lookup.kind === "inviter-left") {
    return (
      <section className="card narrow">
        <h1>Invitation</h1>
        <p>
          Whoever sent this invitation has left the organization, so it no longer works. Ask an
          owner or admin of the organization for a new link.
        </p>
      </section>
    );
  }
  if (lookup.kind === "failed") {
    return (
      <section className="card narrow">
        <h1>Invitation</h1>
        <ErrorText error={lookup.message} />
      </section>
    );
  }

  const { invitation } = lookup;
  // Auth refuses to accept any other role (00012 D1). Such an invitation was
  // made before that rule.
  if (invitation.role !== "member") {
    return (
      <section className="card narrow">
        <h1>Invitation</h1>
        <p>
          This invitation is for the {invitation.role} role, and invitations now admit only as a
          member. Ask an owner or admin of {invitation.organizationName} for a new link.
        </p>
      </section>
    );
  }
  return (
    <section className="card narrow">
      <h1>Join {invitation.organizationName}</h1>
      {declined ? (
        <p>You declined this invitation.</p>
      ) : (
        <>
          <p>
            {invitation.inviterEmail} invited {invitation.email} to join as {invitation.role}.
          </p>
          <div className="row">
            <button onClick={() => join(invitation.organizationId)} disabled={pending}>
              Join
            </button>
            <button className="ghost" onClick={decline} disabled={pending}>
              Decline
            </button>
          </div>
          <ErrorText error={error} />
        </>
      )}
    </section>
  );
}
