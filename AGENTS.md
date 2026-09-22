# Agent Guide

This file holds instructions for coding agents working in this repository.

<!-- devloop:design-judgment v1 sha:0731ae6f -->

## Design Judgment

**Default to the minimal necessary complexity.** Prefer the smallest design that satisfies the current product need, fits the existing architecture, and leaves a clear path to extend later. Be suspicious of abstractions, invariants, retries, background machinery, or schema constraints that mostly exist to make the design feel complete rather than to solve a present problem.

Interpret "smallest design" architecturally, not as the fewest changed lines. First identify the current product need and any invariant it actually requires; then choose the smallest design that satisfies those, fits the existing architecture, and does not move away from a recorded architectural direction. A locally small patch that weakens the required invariant, creates a parallel path, or adds a temporary abstraction already known to be replaced is not minimal. If the stronger invariant is not a present product need, prefer explicitly tolerating and deferring the problem over building machinery for completeness or shipping a partial solution with a stronger claim.

Operator direction sets the product constraints, but it does not remove the agent's obligation to exercise engineering judgment. Surface and argue when a requested direction would conflict with the existing architecture or a recorded direction, or would spend durable complexity on a low-value problem; once the operator decides with that trade-off visible, follow the decision.

**Argue with numbers, and audit your own counter-proposal.** When pushing back on a proposed design, the minimality principles above are priors to test, not verdicts to cite: quantify the binding constraint before judging a design over- or under-built. Then red-team your alternative as hard as the proposal, against the same conditions. A position that needs a new compensating patch each time it is challenged is the escalating-commitment signal from Debugging Discipline at design scope — and it applies to both sides of the discussion. When it is your position, stop defending it and re-derive from scratch; when it is the operator's, your job is to help them break the loop with evidence, not to concede to authority. If neither design survives and the discussion stalls, drop both: enumerate the actual requirements together and derive a solution from those.

**Weigh every edge case before fixing it.** Consider these factors together:

- **Likelihood** — how often does this happen in the current product shape?
- **Impact** — what actually happens to the user or operator?
- **Recoverability** — can they recover with existing UI or an obvious manual action?
- **Cost** — how much implementation, testing, documentation, and future constraint does the fix add?

Tolerate or defer a case when the four factors together show that the fix costs more than the failure and required invariants remain satisfied. Low likelihood alone cannot justify deferring a high-impact, unrecoverable failure. Don't introduce durable complexity to make rare behavior tidy. Call out the trade-off in the handoff so the operator can decide if the bar moves.

**Prefer deriving values from authoritative inputs over maintaining additional mutable copies.** Keep domain calculations independent of caching and scheduling details; justify any required coupling.

**Fail loud > self-heal.** Prefer a single attempt that exposes failure and gives the operator a useful recovery hint. Don't add retry loops, sequencing tricks, or duplicate-execution guards merely to make the operation appear seamless.

**Implementation complexity must earn its place.** Add a status value, index, retry, rollback, or layer only when a _present_ feature needs it. The cost of a wrong abstraction is paid by every later reader; the cost of a right abstraction deferred by one week is usually nothing.
<!-- /devloop:design-judgment -->

<!-- devloop:debugging-discipline v2 sha:568414f5 -->

## Debugging Discipline

**A failed fix is evidence your mental model is incomplete, not yet evidence the design is wrong.** Retry smaller and more carefully. The failure mode to watch for is **escalating commitment under uncertainty**: repeated failed guesses can make a rewrite feel justified, when the fix usually exists at a smaller scope you have not located yet.

- **At a dependency boundary, read the dependency's source before changing your approach.** The source is on disk. Grep for the handler you suspect instead of theorizing from symptoms.
- **Enumerate every input that could produce the symptom before concluding any one is _the_ cause.** Stopping one doesn't stop the others. For event-driven code this means listing the full family that can fire, not the first one that looks plausible.
- **Symptom-based theories are hypotheses, not conclusions.** Before editing, name what you think is happening and what would falsify it. If the fix fails, treat the hypothesis as incomplete or unproven — narrow with a log, source-reading, or a smaller repro before editing again.
- **"Simple" or "minimal" means the smallest sufficient fix with the feature's requirements held constant.** Replacing a broken inline editor with a cruder prompt removes the interaction instead of fixing it. Starting over or switching libraries does not establish that you understood or fixed the cause. Redesign when evidence says the design is wrong, not merely because repeated attempts failed.

<!-- /devloop:debugging-discipline -->

<!-- devloop:test-design v1 sha:18cde2ed -->

## Test Design

- **TDD for behavior changes.** Smallest meaningful failing test first; tests are part of the design.
- **Test at the lowest useful level** with the real contract for that surface: state transitions for domain logic, request/response for HTTP handlers, auth boundaries for identity, user-visible state changes for UI. Cover unhappy paths (authorization failures, stale versions, malformed input, expiry, retries, idempotency, boundaries).
- **Avoid over-stubbing.** A test that passes while real integration fails is worse than no test. When a unit test needs heavy mocking, prefer a narrower pure-helper test, a handler-level test with real local collaborators, or a smoke check.
- **Skipping tests is an explicit engineering call** — justify it with the reason, what verification you ran instead, and what test to add if the surface grows.

<!-- /devloop:test-design -->

<!-- devloop:writing-style v2 sha:ae3d3905 -->

## Writing Style

### Comments

- **Default to no comment.** Only when the _why_ is non-obvious — a hidden constraint, subtle invariant, workaround for a specific bug, behavior that would surprise a reader.
- Don't restate _what_ the code does; identifiers do that.
- Don't reference the current task, fix, or callers (`// used by X`, `// added for the Y flow`, `// handles the case from issue #N`). That belongs in the PR description and rots.
- **One comment, one decision.** Explain the non-obvious reason for the code beside it. Let that explanation wrap naturally. Split independent explanations and place each beside the code it explains. Move broader background to the relevant guide; retain the local context needed to change the code safely.
- **No banner or decorative blocks.** Avoid comments that serve only as visual headings or repeat the name of the code below them.
- Keep public API documentation where developers using the API will find it, including directly above the function or type definition when appropriate.

### Documentation prose

Write docs, issue bodies, and PR/issue comments for a human reviewer, not for density.

- Short sentences, one idea each. Plain wording is not weaker wording: keep the exact claims, names, and invariants, and drop only the compression.
- Don't stack qualifications into em-dash chains or nested parentheticals. Give each qualification its own sentence, or promote the set to a list.
- When prose enumerates cases, obligations, or layers, use a list. A reader should check items off, not parse them out of a paragraph.
- Status headers state a document's current standing only. Revision narration ("revised after review pass N…") is churn; git history already carries it.

### Shell output

- Single space between args; never pad to align value columns. Padded whitespace reads as a typo and costs reader doubt.

<!-- /devloop:writing-style -->

<!-- devloop:commit-conventions v2 sha:761ae1a5 -->

## Commits

Use [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) for **all** commits, including bot-emitted: `<type>(<scope>): <subject>`.

- `feat(<scope>):` — user-visible new behavior.
- `fix(<scope>):` — bug fix.
- `refactor(<scope>):` — restructure code without changing behavior.
- `test(<scope>):` — add or update tests.
- `perf(<scope>):` — improve performance.
- `chore(<scope>):` — version bumps and tooling maintenance.
- `ci(<scope>):` — only for CI workflow changes.
- `docs(<scope>):` — docs-only.

Scopes are this repository's own top-level units. Use a combined `chore(a,b): …` form only when a change genuinely spans both.

- Summary in imperative mood, lower case, no trailing period.
- Body explains _why_, not _what_. The diff shows what changed.
- One logical change per commit. Mechanical changes such as a reformat go in their own commit.
- Never `--amend` unless explicitly asked; create a new commit. Hook failure means the commit didn't happen, so an amend would modify the _previous_ commit and may destroy work.
- Never `--no-verify` to skip hooks. If a commit or hook fails, inspect and report the failure, then fix its cause before retrying.

<!-- /devloop:commit-conventions -->

<!-- devloop:handoff-contract v1 sha:f0253f90 -->

## Handoff

Every task ends with a handoff. Reporting what changed is the easy half; the obligations below are the half that gets dropped, and each one exists because the operator cannot recover it from the diff.

- **What changed, and what you verified.** Name the commands you ran and their results. State plainly what you could not verify and why — an unavailable surface, a check that needs a browser, an environment you don't have. A passing type-check is not evidence that a feature works.
- **Trade-offs you took deliberately.** An edge case you tolerated, a problem you deferred, a stronger invariant you decided the present need didn't require. Say so, so the operator can move the bar if they disagree.
- **Tests you skipped.** The reason, the verification you ran instead, and the test to add if the surface grows.
- **Drift you noticed.** Documentation that no longer matches the tree, a convention the code has quietly stopped following, a stale assumption in the task itself. Surface it even when it was not your task to fix.
- **Side effects you caused.** A logic fix that changes an error state, a validation message that now reads badly, a behavior change a reader of the diff would not predict.
- **Changes you did not make.** Files in the worktree you didn't touch, and anything you left alone because it looked user-owned.

State these even when they are unflattering, and especially when nobody asked. A handoff that only reports success transfers no information.
<!-- /devloop:handoff-contract -->
