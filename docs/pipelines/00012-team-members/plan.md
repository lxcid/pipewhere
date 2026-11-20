# Plan 00012 — Inviting and managing an organization's members

Built in one PR, #53, for #52.

## Steps

1. Auth: `requireEmailVerificationOnInvitation: false`, per D1. Auth's test that refused an unverified invitee now has that invitee join. API's `add_member` test helper and Auth's token test stop verifying the invitee's email. Done.
2. API: refuse a member and a key with 403 on Mastodon's authorize and connect, and on the settings update, per D2 and D3. The checks live in `access.rs` as `OrgCaller::require_manager`, beside 00010 D4's check, which moved there as `require_member`. Done.
3. API tests: each refusal, an admin and an owner passing, and a role change taking effect on the next request with the same token. Done.
4. Web: move the sign-in form, and the wait for the first session answer, out of the home page so the invitation page can use them. Done.
5. Web: the invitation page, `/invite/[invitationId]`. Done.
6. Web: the organization card, with invite, pending invitations, role changes, removal, leaving, and renaming. Channels and API keys hide what the role cannot use, per D4. Done.
7. README: how to invite someone, and what each role may do. Done.
8. Check every interaction in a browser against a local stack. Done.

## Verification

### Commands

Run at `abf69ca`, from the repository root unless noted:

- `pnpm --filter @pipewhere/auth test`: 20 pass. An invitee whose email is not verified joins through the invitation. Another address is refused with 403. An unverified user is refused when listing their invitations, so they cannot find an invitation's id that way.
- `cargo test -p pipewhere-api --test organization_roles`: 3 pass. A member and a key get 403 `forbidden` on starting and finishing a Mastodon connection and on changing settings. An owner and an admin get past the check, to the body's own error. A refused member changes nothing. A member made admin, then member again, gets the matching answer on the next request with the same token.
- `cargo test -p pipewhere-api`, every test binary: pass, with one exception below. `settings.rs` now has an admin change the default tolerance, since D2 overturns 00003 D11's rule that any member or key may.

- `cargo test -p pipewhere-api --lib`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo fmt --check`: clean.
- `pnpm --filter @pipewhere/web test`: 39 pass. `roles.test.ts` covers which roles manage, a comma-separated role, which roles each role may grant, whom an admin may change, the invitation link, and which invitations count as pending.
- `pnpm --filter @pipewhere/web typecheck`: clean.
- `pnpm --filter @pipewhere/web build`: passes. As in 00011, Next.js 16.4.2 first stops with a notice about a known security advisory, and passes when run again.

### Failures outside this change

These were seen on the builder's machine, macOS on Darwin 27, as in 00010:

- `mastodon_publishing::an_instance_that_never_answers_the_connection_refuses` fails in its own setup. It expects the kernel to drop a connection silently once a listener's accept queue is full. This kernel resets it instead.
- `publication_transitions::a_retry_waits_for_another_transaction_holding_its_key` failed once in a parallel run because Auth did not answer `/health` within 10 seconds. It passed when run alone, and in the full run.

### Browser checks against a local stack

Verified at `abf69ca`. The stack ran under Compose project `pipewhere-check` on the builder's machine, with its own volumes. It ran PostgreSQL, the migrations, Auth, API, and Web. Worker did not run: its image was still compiling after 30 minutes, and nothing checked here reaches it. A script drove headless Chrome 157 through the DevTools protocol, with one browser context per person. Every address was on a per-run `example.com` subdomain.

All 43 checks passed:

- An owner sees Rename, the invite form with every role, the connect form, the API key form, and Leave on their own row.
- An invitation's link is on Web's own origin. The pending row shows its address, role, and expiry.
- A signed-out visitor to the link is asked to sign in with the invited address. After signing up, which leaves the email unverified, they see the inviter and the role, join, and land on the organization.
- A member sees no Rename, invite form, connect form, or API keys. They are told why for channels and keys, cannot change or remove the owner, may leave, and still see Publications.
- An accepted invitation leaves the pending list.
- An owner makes a member an admin. After reloading, the admin sees Rename and the connect form, is offered member and admin but not owner, still cannot change the owner, and still sees no API keys.
- An admin invites. Someone signed in with another address is told the invitation is for another address.
- An owner cancels an invitation, and its link then says it was answered, cancelled, or has expired.
- A declined invitation does not admit.
- An owner renames the organization and its slug.
- The only owner cannot leave, and sees Better Auth's reason.
- A member who leaves returns to the organization list, without the organization. The owner sees them gone.
- An owner removes a member, who then sees "This organization does not exist, or you are not a member."
- No sideways scroll at 390 pixels wide. No console errors for any of the five people.

Screenshots from that run, all of fixture data:

- [The owner's view, with a pending invitation](evidence/owner-invited.png)
- [The invitation page](evidence/invitation.png)
- [A member's view](evidence/member-view.png)
- [An invitation for another address](evidence/other-address.png)
- [The only owner refused when leaving](evidence/only-owner.png)
- [Phone width, as an admin](evidence/phone.png)

### Not verified

- The light colour scheme. Headless Chrome followed the machine's dark scheme in every screenshot.
- Browsers other than Chrome.
- Copy link writing to the clipboard. The script read the link from the page instead.
- A role changed in another tab showing without a reload or a return to the window. D4 does not promise it.
