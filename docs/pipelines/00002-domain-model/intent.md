---
status: draft
---

# Intent 00002 — Domain model and publication state machine

## Problem

Scheduling a post looks simple. It goes wrong in a small number of specific ways, and the model decides each of them, not the code around it. A question the model leaves open is answered by whichever pipeline meets it first.

- **One piece of writing goes to several channels, and each channel has its own outcome.** A post can go out on Mastodon and fail on Threads. A status summarised across channels hides that failure. Retrying the whole post re-sends it to the channels where it already succeeded.
- **Content changes after it is scheduled.** The operator needs to know exactly what goes out at 9am. Editing the underlying content must not silently rewrite what is already scheduled. A published post must stay a record of what was actually sent.
- **Some outcomes are unknown.** A provider call can be sent and never answered. The post may be live, or it may not. A duplicate public post is unrecoverable in the way that counts, because people saw it. A missed post is recoverable by publishing it. A model that cannot represent "we do not know" resolves that asymmetry the wrong way by default. It reports a failure the operator retries into a duplicate, or a success that never happened.
- **Requests are retried.** Agents and scripts retry after a timeout. A retried request that schedules the same post twice creates a duplicate before any provider is involved.
- **Edits and cancellations race the publish.** Once a provider call has started, an edit or a cancel can no longer stop it. If the model does not say when a post stops being editable, an edit can produce a public post the system has no record of.
- **Time is ambiguous.** Posting slots are local times on a weekly pattern, and local offsets change twice a year. It is unclear whether a queued post's time is fixed when it joins the queue, or moves when the slots change or an earlier post is removed. A post whose time passes while the system is down raises a second question: publish it late, or not at all.
- **Channels stop being able to publish.** Credentials expire, permissions are revoked, and accounts are removed on the provider's side. The operator usually learns this from a post that failed at its scheduled time.
- **Channels on the same provider differ.** Each Mastodon instance sets its own character and media limits, so one channel accepts a post that another rejects. Found at publish time, that failure arrives at 9am. Found when the post is scheduled, it arrives in the response to the request that caused it.
- **Not every actor should publish unreviewed.** Humans, agents, and automations all act through the API. Some are trusted to fill queues on their own. Others need someone to review their work first. Review that arrives after a post's intended time must not publish it late.
- **One person or a team.** Pipewhere must work for a single operator and for a team sharing the same channels. In a team, not everyone who writes should publish, and not everyone who publishes should connect channels or manage members. For a single operator, none of that may add steps.

## Proposed outcome

A domain model small enough to hold in one's head, that answers the operational questions an operator actually asks:

- Publish this now, to these channels.
- Schedule this for a specific time.
- Add this to the next posting slot.
- Change or cancel this before it goes out.
- What is scheduled this week?
- Did this go out, and where can I see it?
- Which posts failed, and why?
- Retry what failed, without re-posting what succeeded.
- Which channels cannot publish right now?

Every entity in the model traces to one of those questions. Anything that does not is deferred, with the condition that would bring it back written down.

The model is done when:

- Each entity and state has one name, and it means the same thing in REST, MCP, CLI, and web.
- For any post on any channel, the operator can read what will go out, or what went out.
- The record of what went out never changes after it is published.
- An unknown outcome is shown as unknown. It is never reported as a failure or as a success.
- No automatic path publishes the same post to the same channel twice. That covers retries of a provider call, recovery after a restart, and a client retrying its request.
- A post a channel cannot accept is rejected when it is scheduled, whichever client scheduled it.
- Why a post failed is answerable through the API, without raw server logs.
- One operator can run Pipewhere alone, and a team can share it with different permissions, on the same model.

Not part of this problem:

- Analytics and engagement metrics.
- Recurring and evergreen posts.
- Replies, comments, and a social inbox.
- Editing or deleting a post on the network after it is published.
- Dependencies between posts, including multi-part threads.

## Affected users and systems

- **Every interface**: REST, MCP, CLI, and web. They share one application layer and therefore one vocabulary.
- **Service actors**: agents, CI jobs, automations, and integrations acting through the API. Each is a principal of its own, not a person's credential. "May draft and schedule but may not publish now" has to be expressible for them. They also retry requests, so a retried request has to be safe.
- **Provider adapters**, whose differences the model must represent rather than flatten.
- **Worker workflows**, which carry out schedules but do not own them.
- **Operators**, who resolve unknown outcomes and reconnect channels by hand.
- **Team members**, who share channels but not every responsibility. Some write, some publish, and some manage channels and members.

## Constraints

- Private by default. Nothing becomes public except by publishing it to a channel. There is no generic public/private flag.
- PostgreSQL owns business records, including what is scheduled. Restate holds execution state only, per [infrastructure D4](../00001-infrastructure/spec.md). Losing Restate's state must not lose a schedule or publish anything twice.
- Pipewhere does not become an authentication system. Users, sessions, organizations, members, roles, invitations, and API keys stay in Better Auth, inside Auth, per [infrastructure D2](../00001-infrastructure/spec.md).
- The first two providers are Mastodon and Threads. They differ in ways the model must hold:
  - Mastodon publishes in one call. Threads creates a container, waits for it to be ready, and then publishes it.
  - Both process media asynchronously before a post can use it.
  - Threads access tokens expire and must be refreshed. Mastodon tokens usually do not expire, but an instance can configure them to.
  - Each Mastodon instance sets its own limits, so channels on the same provider have different limits.
  - A Mastodon account is identified by its instance as well as its id. The same id can exist on two instances.
  - Threads caps how many posts an account may publish in 24 hours.
- Multi-tenancy has to remain possible without a migration that rewrites every foreign key.
- Organization and channel modelling must not assume a hosted deployment.

## Open questions

For the providers, each of which carries lead time:

- Threads' behaviour when `threads_publish` is re-issued with an already-published `creation_id`, and whether a container's status reliably shows it was published after a publish call timed out.
- Whether Threads API access to an owned account needs full app review, or whether development-mode tester access suffices. Settle this regardless of when this pipeline starts.
- Whether the Mastodon instances in scope expire access tokens.
