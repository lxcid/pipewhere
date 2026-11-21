# Plan 00012 — Inviting and managing an organization's members

Built in one PR, #53, for #52.

## Steps

1. Auth: `requireEmailVerificationOnInvitation: false`, per D1. Auth's test that refused an unverified invitee now has that invitee join. Done.
2. Auth: refuse creating or accepting an invitation for any role but member, through `beforeCreateInvitation` and `beforeAcceptInvitation`, per D1. API's `add_member` test helper and Auth's token test invite as member, then change the role. Done.
3. API: refuse a member and a key with 403 on Mastodon's authorize and connect, and on the settings update, per D2 and D3. The checks live in `access.rs` as `OrgCaller::require_manager`, beside 00010 D4's check, which moved there as `require_member`. Done.
4. API tests: each refusal, an admin and an owner passing, and a role change taking effect on the next request with the same token. Done.
5. Web: move the sign-in form, and the wait for the first session answer, out of the home page so the invitation page can use them. Done.
6. Web: the invitation page, `/invite/[invitationId]`, including an inviter who has left. Done.
7. Web: the organization card, with invite, pending invitations, role changes, removal, leaving, and renaming. Channels and API keys hide what the role cannot use, per D4. Done.
8. README: how to invite someone, and what each role may do. Done.
9. Check every interaction in a browser against a local stack. Done.

## Verification

### Commands

Run at `0a850f6`, from the repository root, unless a line names another commit. Only the invitation page changed in `f391d15`, and Web's tests and typecheck were run again there:

- `pnpm --filter @pipewhere/auth test`: 22 pass.
  - An invitee whose email is not verified joins through the invitation.
  - Another address is refused with 403.
  - An unverified user is refused when listing their invitations, so they cannot find an invitation's id that way.
  - An invitation for `admin`, `owner`, `member, owner`, or `member,admin` is refused with 400, and none is stored.
  - An invitation stored with the owner role is refused when accepted, and no membership is made.
- `cargo test -p pipewhere-api --no-fail-fast`, at `a4b775a`: every test passes except one, listed below. API's code has not changed since. At `0a850f6`, `organization_roles`, `access`, `settings`, and `publication_approval` were run again, since they invite through Auth: 3, 10, 5, and 4 pass.
  - `organization_roles.rs`: a member and a key get 403 `forbidden` on starting and finishing a Mastodon connection, and on changing settings. An owner and an admin get past the check, to the body's own error. A refused member changes nothing. A member made admin, then member again, gets the matching answer on the next request with the same token.
  - `settings.rs`: an admin changes the default tolerance, since D2 overturns 00003 D11's rule that any member or key may.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check`: clean.
- `pnpm --filter @pipewhere/web test`: 39 pass. `roles.test.ts` covers:
  - which roles manage, including a comma-separated role;
  - which roles each role may grant, and whom an admin may change;
  - the invitation link;
  - which invitations count as pending.
- `pnpm --filter @pipewhere/web typecheck`: clean.
- `pnpm --filter @pipewhere/web build`: passed at `abf69ca`, and the check stack's Web image built at `f391d15`. As in 00011, Next.js 16.4.2 first stops with a notice about a known security advisory, and passes when run again.

### Failures outside this change

These were seen on the builder's machine, macOS on Darwin 27, as in 00010:

- `mastodon_publishing::an_instance_that_never_answers_the_connection_refuses` fails in its own setup. It expects the kernel to drop a connection silently once a listener's accept queue is full. This kernel resets it instead.
- `publication_transitions::a_retry_waits_for_another_transaction_holding_its_key` failed once in a parallel run because Auth did not answer `/health` within 10 seconds. It passed when run alone, and in the full run.

### Browser checks against a local stack

Verified at `f391d15`. The stack ran under Compose project `pipewhere-check` on the builder's machine, with its own volumes. It ran PostgreSQL, the migrations, Auth, API, and Web. Worker did not run: its image was still compiling after 30 minutes, and nothing checked here reaches it. A script drove headless Chrome 157 through the DevTools protocol, with one browser context per person. Every address was on a per-run `example.com` subdomain. The script waits 11 seconds between sign-ups, because Better Auth allows 3 sign-ups from one address until 10 seconds pass without one.

All 51 checks passed:

- An owner sees Rename, the invite form without a role, the connect form, the API key form, and Leave on their own row.
- An invitation's link is on Web's own origin. The pending row shows its address, role, and expiry.
- A signed-out visitor to the link is asked to sign in with the invited address. After signing up, which leaves the email unverified, they see the inviter and the role, join, and land on the organization.
- A member sees no Rename, invite form, connect form, or API keys. They are told why for channels and keys, cannot change or remove the owner, may leave, and still see Publications.
- An accepted invitation leaves the pending list.
- An owner makes a member an admin. After reloading, the admin sees Rename and the connect form, still cannot change the owner, and still sees no API keys.
- An invitee joins as a member. An owner is offered every role for them. An admin is offered member and admin, not owner, for another member.
- An admin invites. Someone signed in with another address is told the invitation is for another address.
- An owner cancels an invitation, and its link then says it was answered, cancelled, or has expired.
- A declined invitation does not admit.
- An owner renames the organization and its slug.
- The only owner cannot leave, and sees Better Auth's reason.
- A member who leaves returns to the organization list, without the organization. The owner sees them gone.
- An owner removes a member, who then sees "This organization does not exist, or you are not a member."
- An invitation sent by the removed admin tells its invitee that the inviter has left. The owner still sees it pending, to cancel.
- An invitation whose stored role was set to owner in `auth.invitation` asks its invitee for a new link, without Join.
- No sideways scroll at 390 pixels wide. No console errors for any of the seven people.

An earlier run at the same commit timed out at Carol's check that a cancelled link no longer works. The script did not capture her page, so the cause is unknown. The next run passed it.

Screenshots from that run, all of fixture data. Headless Chrome followed the machine's colour scheme, which was light for this run and dark for the run at `abf69ca`:

- [The owner's view, with a pending invitation](evidence/owner-invited.png)
- [The invitation page](evidence/invitation.png)
- [A member's view](evidence/member-view.png)
- [An invitation for another address](evidence/other-address.png)
- [An invitation whose inviter left](evidence/inviter-left.png)
- [The only owner refused when leaving](evidence/only-owner.png)
- [Phone width, as an admin](evidence/phone.png)

### Not verified

- Browsers other than Chrome.
- Copy link writing to the clipboard. The script read the link from the page instead.
- A role changed in another tab showing without a reload or a return to the window. D4 does not promise it.
- A reload that fails for a reason other than missing membership keeping the page, and a lookup that never reaches Auth. Both need Auth to fail on request, which the script could not arrange.
- More than 100 invitations in one organization. D4 tolerates it.
