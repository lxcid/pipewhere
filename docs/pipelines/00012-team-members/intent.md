---
status: in-progress
---

# Intent 00012 — Inviting and managing an organization's members

## Problem

An organization has one person in practice. Web can create an organization and list its members, but nobody else can join it:

- Web cannot invite anyone, and Auth cannot send email.
- Joining needs a verified email, per [infrastructure D2](../00001-infrastructure/spec.md). Verifying one needs email too.

Web also cannot change a member's role, remove a member, leave an organization, or rename one. And API treats every member the same, so a newly invited person could connect or replace the organization's channels.

Draft [intent 00006](../00006-team-review/intent.md) describes team permissions and review in full. This intent splits off the first part the operator needs, so it can be approved and built while the rest of 00006 stays a draft.

## Proposed outcome

From an organization's page in Web:

- An owner or admin invites someone by email, with a role, and copies an invitation link to send them. They see pending invitations and can cancel one.
- The invited person opens the link, signs up or signs in with the invited address, and joins. No email is sent and none needs verifying.
- An owner or admin changes a member's role or removes a member, within Better Auth's rules: only an owner makes someone an owner, and an organization always keeps an owner.
- A member leaves an organization, unless they are its only owner.
- An owner or admin renames the organization.

In API:

- A member may do all publication work: write, schedule, edit, cancel, retry, resolve, and approve.
- Only an owner or admin may connect a channel or change the organization's settings. A member who tries gets forbidden.
- A role change or a removal takes effect on the member's next request.

This is true when a second person joins through an invitation link in Web, can publish but cannot connect a channel as a member, and can connect one after an owner makes them an admin.

## Affected users and systems

Owners, admins, and members; Auth, API, and Web.

## Constraints

- Auth owns organizations, members, roles, and invitations, per infrastructure D2. Web calls Better Auth's endpoints for them and does not become a privileged backend.
- Roles are Better Auth's three defaults: owner, admin, and member. No custom roles.
- No email delivery. The operator chose invitation links over email.
- Review policy for members, approval after a planned time, and per-key permissions stay with 00006.
- Organization deletion stays off.

## Open questions

None.
