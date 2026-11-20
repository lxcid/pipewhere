# Plan 00012 — Inviting and managing an organization's members

Built in one PR, #52's.

## Steps

1. Auth: `requireEmailVerificationOnInvitation: false`, per D1. Auth's test that refuses an unverified invitee becomes one where that invitee joins. API's `add_member` test helper stops verifying the invitee's email.
2. API: refuse a member and a key with 403 on Mastodon's authorize and connect, and on the settings update, per D2 and D3. The checks move to `access.rs` beside `OrgCaller`, with 00010 D4's member-only check.
3. API tests: each refusal, an admin and an owner passing, and a role change taking effect on the next request with the same token.
4. Web: move the sign-in form out of the home page so the invitation page can use it.
5. Web: the invitation page, `/invite/[invitationId]`.
6. Web: the organization page's members card, with invite, pending invitations, role changes, removal, leaving, and renaming. Channels and API keys hide what the role cannot use, per D4.
7. README: how to invite someone, and what each role may do.
8. Check every interaction in a browser against a local stack.

## Verification

Not yet run.
