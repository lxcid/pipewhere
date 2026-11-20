"use client";

import { useRouter } from "next/navigation";
import { useState, type FormEvent } from "react";

import { useAuth } from "./auth-client";
import {
  assignableRoles,
  canChange,
  invitationLink,
  manages,
  pendingInvitations,
  type Role,
} from "./roles";
import { ErrorText, useAction } from "./ui";

export interface Member {
  id: string;
  userId: string;
  role: string;
  user: { name: string; email: string };
}

export interface Organization {
  id: string;
  name: string;
  slug: string;
  members: Member[];
  invitations: {
    id: string;
    email: string;
    role: string;
    status: string;
    expiresAt: string | Date;
  }[];
}

interface Props {
  organization: Organization;
  me: Member;
  reload: () => Promise<unknown>;
}

export function OrganizationCard({ organization, me, reload }: Props) {
  return (
    <section className="card">
      {manages(me.role) ? (
        <Rename organization={organization} reload={reload} />
      ) : (
        <h1>{organization.name}</h1>
      )}
      <p className="muted">
        <code>{organization.id}</code> · {organization.slug}
      </p>
      <h2>Members</h2>
      <ul className="list">
        {organization.members.map((member) => (
          <MemberRow
            key={member.id}
            organizationId={organization.id}
            member={member}
            me={me}
            reload={reload}
          />
        ))}
      </ul>
      {manages(me.role) && <Invitations organization={organization} me={me} reload={reload} />}
    </section>
  );
}

function Rename({ organization, reload }: Omit<Props, "me">) {
  const { client } = useAuth();
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(organization.name);
  const [slug, setSlug] = useState(organization.slug);
  const { pending, error, run } = useAction();

  function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    run(async () => {
      const result = await client.organization.update({
        organizationId: organization.id,
        data: { name, slug },
      });
      if (result.error) return result;
      setEditing(false);
      await reload();
    });
  }

  if (!editing) {
    return (
      <div className="heading">
        <h1>{organization.name}</h1>
        <button
          className="ghost"
          onClick={() => {
            setName(organization.name);
            setSlug(organization.slug);
            setEditing(true);
          }}
        >
          Rename
        </button>
      </div>
    );
  }
  return (
    <form onSubmit={save}>
      <div className="row">
        <input
          aria-label="Name"
          value={name}
          required
          onChange={(event) => setName(event.target.value)}
        />
        <input
          aria-label="Slug"
          value={slug}
          required
          pattern="[a-z0-9-]+"
          onChange={(event) => setSlug(event.target.value)}
        />
        <button type="submit" disabled={pending}>
          Save
        </button>
        <button type="button" className="ghost" onClick={() => setEditing(false)}>
          Cancel
        </button>
      </div>
      <ErrorText error={error} />
    </form>
  );
}

function MemberRow({
  organizationId,
  member,
  me,
  reload,
}: {
  organizationId: string;
  member: Member;
  me: Member;
  reload: () => Promise<unknown>;
}) {
  const { client } = useAuth();
  const router = useRouter();
  const [confirming, setConfirming] = useState(false);
  const { pending, error, run } = useAction();
  const self = member.id === me.id;
  const changeable = !self && canChange(me.role, member.role);

  function changeRole(role: Role) {
    run(async () => {
      const result = await client.organization.updateMemberRole({
        organizationId,
        memberId: member.id,
        role,
      });
      if (result.error) return result;
      await reload();
    });
  }

  function remove() {
    run(async () => {
      const result = self
        ? await client.organization.leave({ organizationId })
        : await client.organization.removeMember({ organizationId, memberIdOrEmail: member.id });
      if (result.error) {
        setConfirming(false);
        return result;
      }
      if (self) router.push("/");
      else await reload();
    });
  }

  return (
    <li>
      <span>
        {member.user.name} <span className="muted">{member.user.email}</span>
        {self && <span className="muted"> (you)</span>}
      </span>
      {confirming ? (
        <span className="row tight">
          <span>
            {self ? "Leave this organization?" : `Remove ${member.user.email}?`} Only an invitation
            brings {self ? "you" : "them"} back.
          </span>
          <button onClick={remove} disabled={pending}>
            {self ? "Leave" : "Remove"}
          </button>
          <button className="ghost" onClick={() => setConfirming(false)} disabled={pending}>
            Keep
          </button>
        </span>
      ) : (
        <span className="row tight">
          {changeable ? (
            <select
              aria-label={`Role of ${member.user.email}`}
              value={member.role}
              disabled={pending}
              onChange={(event) => changeRole(event.target.value as Role)}
            >
              {assignableRoles(me.role).map((role) => (
                <option key={role} value={role}>
                  {role}
                </option>
              ))}
            </select>
          ) : (
            <span className="muted">{member.role}</span>
          )}
          {(self || changeable) && (
            <button className="ghost" onClick={() => setConfirming(true)} disabled={pending}>
              {self ? "Leave" : "Remove"}
            </button>
          )}
        </span>
      )}
      <ErrorText error={error} />
    </li>
  );
}

function Invitations({ organization, me, reload }: Props) {
  const { client } = useAuth();
  const [email, setEmail] = useState("");
  const [role, setRole] = useState<Role>("member");
  const [copied, setCopied] = useState<string | null>(null);
  const { pending, error, run } = useAction();
  const waiting = pendingInvitations(organization.invitations, new Date());

  function invite(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    run(async () => {
      const result = await client.organization.inviteMember({
        organizationId: organization.id,
        email,
        role,
      });
      if (result.error) return result;
      setEmail("");
      setRole("member");
      await reload();
    });
  }

  function cancel(invitationId: string) {
    run(async () => {
      const result = await client.organization.cancelInvitation({ invitationId });
      if (result.error) return result;
      await reload();
    });
  }

  function copy(link: string) {
    run(async () => {
      await navigator.clipboard.writeText(link);
      setCopied(link);
    });
  }

  return (
    <>
      <h2>Invite</h2>
      <p className="muted">
        Pipewhere sends no email. Copy the invitation&apos;s link and send it yourself. Whoever
        opens it joins after signing in with the invited address, so send it only to them. It
        expires after 48 hours.
      </p>
      <form onSubmit={invite} className="row">
        <input
          type="email"
          aria-label="Email"
          placeholder="Email"
          value={email}
          required
          onChange={(event) => setEmail(event.target.value)}
        />
        <select
          aria-label="Role"
          value={role}
          onChange={(event) => setRole(event.target.value as Role)}
        >
          {assignableRoles(me.role).map((option) => (
            <option key={option} value={option}>
              {option}
            </option>
          ))}
        </select>
        <button type="submit" disabled={pending}>
          Invite
        </button>
      </form>
      <ErrorText error={error} />
      {waiting.length > 0 && (
        <ul className="list">
          {waiting.map((invitation) => {
            const link = invitationLink(window.location.origin, invitation.id);
            return (
              <li key={invitation.id}>
                <span>
                  {invitation.email} <span className="muted">{invitation.role}</span>
                  <span className="muted">
                    {" "}
                    · expires {new Date(invitation.expiresAt).toLocaleString()}
                  </span>
                </span>
                <code>{link}</code>
                <span className="row tight">
                  <button className="ghost" onClick={() => copy(link)} disabled={pending}>
                    {copied === link ? "Copied" : "Copy link"}
                  </button>
                  <button
                    className="ghost"
                    onClick={() => cancel(invitation.id)}
                    disabled={pending}
                  >
                    Cancel
                  </button>
                </span>
              </li>
            );
          })}
        </ul>
      )}
    </>
  );
}
