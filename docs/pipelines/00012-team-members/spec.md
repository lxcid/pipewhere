# Spec 00012 — Inviting and managing an organization's members

Facts about Better Auth below were read from its source, version 1.7.12, the version Auth and Web pin.

## Decisions

### D1 — An invited person joins through the invitation's link, without a verified email

Overturns: 00001 D2, its rule that joining an organization needs a verified email.

The operator chose on 2026-11-21 to invite through links rather than email. Auth sends no email, and a verified email needs one. Auth therefore sets Better Auth's `requireEmailVerificationOnInvitation` to `false`. It stays explicit, because 00001 D6's id generator would otherwise switch it on.

What still guards an invitation:

- Better Auth accepts an invitation only from a signed-in user whose email matches the invited address.
- The link carries the invitation's id. That id is a UUIDv7, per 00001 D6, with 74 random bits, so it cannot be guessed.
- Better Auth lists a user's invitations only to a user whose email is verified, whatever this setting says. An unverified user cannot find an invitation's id that way.
- An invitation expires after Better Auth's default of 48 hours. An owner or admin can cancel it sooner.

What it gives up: an email address no longer proves who owns it. Someone who holds the link, and signs up with the invited address before the invitee does, joins in the invitee's place. The link can leak where 00001 D6 allows ids to appear, such as logs. If it happens, an owner or admin sees the member and removes them.

The operator accepted that risk. Today Auth listens on `127.0.0.1` only, per the README, so only someone on the operator's machine can reach it.

What would overturn this: Pipewhere gaining email delivery, or a deployment whose Auth people outside the operator's machine can reach.

### D2 — Owners and admins manage the organization; members publish

This sets what each of Better Auth's three roles may do. The operator chose it on 2026-11-21.

In API:

- Any member may do all publication work: write, schedule, edit, cancel, retry, resolve, and approve. This is unchanged.
- Only an owner or admin may start or finish connecting a Mastodon account, or change the organization's settings. A member gets 403 `forbidden`.

Better Auth enforces the rest, and Pipewhere keeps its defaults:

- An owner or admin may invite, change roles, remove members, cancel invitations, and rename the organization.
- Only an owner may make someone an owner, or change an owner's role. Inviting someone as an owner needs an owner too.
- An organization always keeps an owner. Its only owner cannot leave or be demoted.
- Only an owner may create or revoke API keys.
- A member may do none of these. A member may leave.

Any member may list the organization's invitations, and so see their links. A member can already act in the organization, and a link admits only the invited address.

### D3 — API reads the role on each request, and refuses keys by kind

Proposed: awaiting operator approval.

API already reads the caller's role from `auth.member` on every request, per 00001 D2. The check for D2's actions runs inside the handler, after the membership check, so 00001 D10's order holds:

- unauthorized, then not found, then forbidden.

A role change or a removal therefore takes effect on the member's next request, even while their token is valid.

Better Auth stores several roles as one comma-separated string. API treats a member as an owner or admin when any of their roles is one of those. Web and Better Auth's defaults give each member one role, so this only keeps API from refusing a valid record.

An API key gets 403 `forbidden` for D2's actions too. Every key is treated the same, as [00010 D4](../00010-agent-access/spec.md) does for approval. No key has permissions of its own yet, so 00001 D10's key-permission checks, and the Auth endpoint that sets a key's permissions, still have nothing to test. They wait for 00006, which owns per-key permissions.

00001 D10's role checks are built here:

- a member whose role does not permit an action, refused with forbidden
- a role change, while a token is still valid, taking effect on the next request

### D4 — Web offers each person only what their role allows

Proposed: awaiting operator approval.

Web reads the signed-in user's role from the organization's member list. It hides each control that role cannot use. Auth and API still refuse it, so hiding is for clarity, not protection.

- **Invite.** An owner or admin enters an email and a role. Web shows the invitation's link, `/invite/<invitation id>` on Web's own origin, to copy and send. Only an owner is offered the owner role.
- **Pending invitations.** Each one shows its address, role, expiry, and link, with Cancel.
- **Accepting.** The link's page asks a signed-out visitor to sign in or create an account, then shows the organization's name and the inviter, with Join and Decline. A signed-in user whose email differs is told the invitation is for another address, and can sign out.
- **Members.** An owner or admin changes a role or removes a member. Web offers what Better Auth allows, and shows Better Auth's refusal when it refuses anyway, such as removing the only owner.
- **Leaving.** Any member may leave. Web asks to confirm, then returns to the organization list.
- **Renaming.** An owner or admin changes the name and the slug.
- **Channels.** A member sees the channels and their status, without the form that connects one.
- **API keys.** Only an owner sees the form that creates a key and the Revoke buttons.

Web reloads the member list after each change it makes. It does not watch for changes made elsewhere. A person whose role changes in another tab sees it after a reload, and API refuses what the new role cannot do meanwhile.
