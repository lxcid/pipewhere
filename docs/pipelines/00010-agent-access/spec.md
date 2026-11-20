# Spec 00010 — Agent access over MCP

## Decisions

### D1 — An agent's publications wait for a person to approve them

This answers the open question "Before [team review](../00006-team-review/intent.md) exists, may an agent publish without review, or only create publications that wait for a person?"

They wait. A publication an agent creates goes out only after a person approves it. The operator chose this on 2026-11-20, so that the review step exists from the first version rather than being added after an agent has already posted unreviewed.

This adds a state before `scheduled`, which [publication contract D3](../00002-publication-contract/spec.md) allows an approval pipeline to add through its own decision. 00010 is that pipeline for agent-created publications. The state's name, its transitions, and whether it is editable are decided when it is built. So are approval that arrives after the planned time and where the operator approves. Roles, invitations, permission limits, and review policy per actor or per channel stay with 00006.

### D2 — An activity log, not event sourcing

The operator can see every action taken on a publication, who took it, and when. The operator chose this on 2026-11-20 because it gives people confidence in what an agent did on their behalf.

Today, `publication_attempt` records each execution by Worker, but actions taken through API do not leave a record. Create, edit, cancel, retry, and resolve change the `publication` row in place. Neither the actor nor the time is kept, and an edit or retry overwrites the previous body or planned time.

Event sourcing was considered and rejected. It would make events the source of truth and derive publication state from them. These guarantees rest on conditional updates to the publication row:

- the publish boundary (00002 D3 and D4)
- idempotent creates (00002 D13)
- the worker's claim

Each would have to be rebuilt on stream versions, along with the list, the due query, and paging. Its added powers, replaying history into new state and reading state at a past time, answer no present need.

What would overturn this: a feature that needs state reconstructed at a past time, such as undo, which an append-only log cannot provide.

### D3 — The log is an append-only table written with each change

Proposed: awaiting operator approval.

Each action through API writes one row, in the same transaction as the change it records. The row holds the publication, the action, the actor, the time, and the fields the action changed. The `publication` row stays the source of truth for current state. `publication_attempt` stays the record of Worker's executions. The log does not repeat them.

A test checks that each action endpoint writes exactly one row, so an action that forgets to log fails the build rather than leaving a silent gap.

### D4 — `awaiting_approval` comes before `scheduled`, and only a member leaves it

Proposed: awaiting operator approval.

This is the approval state [publication contract D3](../00002-publication-contract/spec.md) lets an approval pipeline add.

- **Who waits.** A publication created with an API key starts in `awaiting_approval`. One created by a member starts in `scheduled`, as before. API counts every API key as an agent. An operator's own script that uses a key is therefore reviewed too. Telling agents from other keys would need a kind of key that Auth does not have.
- **Approve.** `POST …/approve` moves `awaiting_approval` → `scheduled`, keeping the publication's time. Any member may approve, whatever their role. An API key gets 403 `forbidden`, so an agent can never approve its own work or another agent's. Roles that limit who approves belong to 00006.
- **00001 D10.** These are the first actions some caller may not perform, so D10 applies. Its ordering holds: 403 comes only after the membership check, and the key refusals are tested. The gate depends on the kind of caller, member or key, and reads no role or permission. D10's other checks therefore have nothing to test yet. These are the role and key-permission checks, and a permission change taking effect on the next request. The Auth endpoint that sets a key's permissions waits with them for 00006, which introduces roles and permissions.
- **Reject.** Cancel moves `awaiting_approval` → `cancelled`. A separate `rejected` state would record nothing a person can act on until the activity log records who acted.
- **What a key may do.** A key may create and read publications, and cancel one that is `awaiting_approval`. Edit, retry, resolve, and approve answer it 403 `forbidden` in any state, and so does cancelling a `scheduled` or `preparing` publication. Otherwise a key could change text after a member approved it, change a draft while a member reviews it, send a failed publication out again unreviewed, or stop a post a member scheduled or approved. 00010's intent gives an agent nothing beyond creating and reading. Cancelling a draft lets an agent withdraw it. A key may withdraw any waiting draft, not only its own. Telling its own drafts apart needs the creating actor, which arrives with D3's log.
- **Edit.** A member may edit a waiting publication, to fix an agent's text before approving it. Web has no edit screen yet. API allows it.
- **Late approval.** Approval does not check the deadline. A publication approved after its deadline fails at the worker's claim as "Missed its time", per [00003 D1](../00003-scheduling-recovery/spec.md), and nothing stale is posted. Web stops offering Approve once a draft is past its deadline, and says why. Web's list refreshes every 10 seconds while a draft waits, extending [00011 D4](../00011-web-publishing-one-account/spec.md). Approve also checks the deadline when clicked, so a card one refresh behind cannot approve a draft that is already too late. Web judges the deadline by the browser's clock, and Worker by PostgreSQL's. A browser clock running fast can therefore withhold Approve from a draft that could still go out; nothing posts that should not. A draft approved just before its deadline can still miss it, because Worker polls every 5 seconds. Web therefore names the deadline rather than promising the post. Approval that also sets a new time is left until the operator needs it.
- **The worker.** Worker claims only `scheduled` publications, so a waiting one never runs. The overdue list also shows only `scheduled` work, so a waiting publication past its time does not appear there.

A repeated create with the same `Idempotency-Key` returns the first publication as it is now, per 00002 D5. Sending a member's request again with a key, or a key's request again as a member, returns the first result. It does not change its state.
