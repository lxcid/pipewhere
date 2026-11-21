# Spec 00012 — Inviting and managing an organization's members

Facts about Better Auth below were read from its source, version 1.7.12, the version Auth and Web pin.

## Decisions

### D1 — An invited person joins through the invitation's link, without a verified email

Overturns: 00001 D2, its rule that joining an organization needs a verified email.

The operator chose on 2026-11-21 to invite through links rather than email. Auth sends no email, and a verified email needs one. Auth therefore sets Better Auth's `requireEmailVerificationOnInvitation` to `false`. It stays explicit, because 00001 D6's id generator would otherwise switch it on.

What still guards an invitation:

- Better Auth accepts an invitation only from a signed-in user whose email matches the invited address.
- The link carries the invitation's id. That id is a UUIDv7, per 00001 D6, made by `uuid` 10.0.0 through `typeid-js`. It has 43 random bits. Its other 31 bits are a counter that the process keeps and increments for every id, so any other id Auth made reveals it. 43 bits, about 8.8 × 10¹² values, are beyond guessing within the 48 hours an invitation lasts.
- Better Auth lists a user's invitations only to a user whose email is verified, whatever this setting says. An unverified user cannot find an invitation's id that way.
- An invitation expires after Better Auth's default of 48 hours. An owner or admin can cancel it sooner.

What it gives up: an email address no longer proves who owns it. Someone who holds the link, and signs up with the invited address before the invitee does, joins in the invitee's place. The link can leak where 00001 D6 allows ids to appear, such as logs. Every member can also read it, per D2.

So an invitation carries only the member role, as the operator chose on 2026-11-21. Whoever claims one gains no more than any member already holds. An owner or admin gives a higher role after the person joins. The member list shows only the invited address and a name the person chose, which is what someone in the invitee's place would also show. So confirm with the person, outside Pipewhere, that they joined before giving them more. Auth refuses to create an invitation for any other role, through Better Auth's `beforeCreateInvitation` hook. Its `beforeAcceptInvitation` hook refuses accepting one too, for an invitation stored before this rule. That also closes a gap in Better Auth's own check: it looks for the owner role among the untrimmed parts of a comma-separated role, so `member, owner` would pass it and then count as owner.

If someone joins in the invitee's place, an owner or admin sees the member and removes them.

The operator accepted that risk. Today Auth listens on `127.0.0.1` only, per the README, so only someone on the operator's machine can reach it.

What would overturn this: Pipewhere gaining email delivery, or a deployment whose Auth people outside the operator's machine can reach.

### D2 — Owners and admins manage the organization; members publish

Overturns: 00003 D11, its rule that any member and any key may change the organization's default tolerance.

This sets what each of Better Auth's three roles may do. The operator chose it on 2026-11-21. 00003 D11 left the default tolerance open to everyone only until a pipeline built forbidden, and left team permissions to 00006, which this intent splits from.

In API:

- Any member may do all publication work: write, schedule, edit, cancel, retry, resolve, and approve. This is unchanged.
- Only an owner or admin may start or finish connecting a Mastodon account, or change the organization's settings. A member gets 403 `forbidden`.

Better Auth enforces the rest, and Pipewhere keeps its defaults:

- An owner or admin may invite, change roles, remove members, cancel invitations, and rename the organization.
- Only an owner may make someone an owner, or change an owner's role.
- An organization always keeps an owner. Its only owner cannot leave or be demoted. Better Auth counts the owners before it removes or demotes one, without a lock. Two owners who leave or demote each other at the same moment could therefore leave none. This exists on `main` already, through Auth's API, and needs two owners acting within the same instant. It is tolerated. Recovery is an `UPDATE auth.member SET role = 'owner'` for the right member.
- Removing a member takes effect on their next request. Better Auth checks for an existing invitation before it creates one, and inserts a membership without checking for an existing one, both without a lock. Two managers who invite the same address at the same moment, followed by the invitee accepting both, could therefore give that user two memberships. Removing one would leave the other. The member list shows both rows, so the second can be removed too. This is tolerated for the same reasons.
- Only an owner may list, create, or revoke API keys.
- A member may do none of these. A member may leave.

Any member may list the organization's invitations, and so see their links. Better Auth gives them to every member, and D1 keeps that harmless: an invitation admits only as a member.

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

- **Invite.** An owner or admin enters an email. The invitation is for the member role, per D1. Web shows its link, `/invite/<invitation id>` on Web's own origin, to copy and send. Better Auth returns at most 100 of an organization's invitations, whatever their status, in no set order. Past 100, some pending invitations may not show, and cannot be copied or cancelled from Web. They still expire after 48 hours. This is tolerated: an organization this product serves is far from 100 invitations. Removing answered invitations is the fix if one gets there.
- **Pending invitations.** Each one shows its address, role, expiry, and link, with Cancel.
- **Accepting.** The link's page asks a signed-out visitor to sign in or create an account, then shows the organization's name and the inviter, with Join and Decline. A signed-in user whose email differs is told the invitation is for another address, and can sign out. If the inviter has left, Better Auth will not show the invitation, so the page says so and asks for a new link. Owners and admins still see it as pending until they cancel it.
- **Members.** An owner or admin changes a role or removes a member. Web offers what Better Auth allows, and shows Better Auth's refusal when it refuses anyway, such as removing the only owner.
- **Leaving.** Any member may leave. Web asks to confirm, then returns to the organization list.
- **Renaming.** An owner or admin changes the name and the slug.
- **Channels.** A member sees the channels and their status, without the form that connects one.
- **API keys.** Only an owner sees the organization's keys. Anyone else is told that only an owner manages them.

Web reloads the organization after each change it makes. Only Better Auth's answer that the organization does not exist, or that the user is not a member, shows as such. Any other failed reload, including a request that never reaches Auth, keeps the page as it was. The invitation page says it could not reach Auth. Web does not watch for changes made elsewhere. A person whose role changes in another tab sees it after a reload, and API refuses what the new role cannot do meanwhile.
