<!-- devloop:pipelines v1 sha:a96ff457 -->

# Pipelines

A pipeline is one unit of work, from the problem that motivated it to the code that closed it. Everything about that work lives in one directory.

    docs/pipelines/00001-infrastructure/
      intent.md    the problem, in the operator's terms
      spec.md      the decisions taken
      plan.md      the implementation steps

The shape is adapted from [Anthropic's AI-native SDLC playbook](https://claude.com/blog/the-ai-native-sdlc-playbook). We kept the parts that survive a team of one plus agents and dropped the rest. What we dropped is recorded at the bottom, so the decision does not get relitigated every time someone reads the original playbook.

## Ownership

The operator owns the intent and its acceptance. The builder owns the plan. The spec is shared: the operator may set decisions and preferences, and the builder adds the decisions its investigation turns up. The plan changes freely within those bounds. Changing the intent or an approved decision requires returning to the operator.

| File | Owner | Changes during the build |
| --- | --- | --- |
| `intent.md` | operator | the body only through the operator; `status` as the Status section says |
| `spec.md` | shared | the builder adds decisions; approved decisions and preferences change only through the operator |
| `plan.md` | builder | freely, whenever the build shows it is wrong |

The builder is whoever is building, usually an agent. Ownership is authority, not authorship. An agent may draft all three files; ownership decides whose approval a change needs.

The operator steers the implementation without owning the plan. Each kind of direction has one home:

- A boundary the solution must stay within is a constraint in the intent.
- A settled choice, with its reasoning, is a decision in the spec.
- A leaning the builder may challenge is a preference in the spec.

One invariant holds the model together:

**The plan can be wrong. The intent cannot silently change to make the plan right.**

When the build shows the plan is wrong, the builder changes the plan. When it shows the intent or an approved decision is wrong, the builder stops and brings the evidence to the operator.

## Naming

    docs/pipelines/<NNNNN>-<slug>/

Five digits, zero-padded, starting at `00001`. Numbers are assigned when a pipeline is opened and are never reused, including for abandoned pipelines. A number is an identifier. It is not a priority and not a build order.

The slug is lowercase and hyphenated, and it names the work rather than the solution.

## The three files

Ceremony scales with uncertainty and risk. The intent is the durable minimum. A spec or a plan is written when it reduces uncertainty, not as paperwork.

### `intent.md` - always

The problem, in the operator's words, written before a solution exists. It is not edited afterwards to match what was built. An intent rewritten to describe its own implementation has lost the only thing it was for.

An agent may draft it. The operator approves it. Approval is the gate: no spec, no plan, and no code until the frontmatter says `approved`. After approval, only the operator changes its body.

Frontmatter carries `status` and nothing else. Sections: Problem, Proposed outcome, Affected users and systems, Constraints, Open questions. The proposed outcome says how to tell when the outcome is true, because that is what the build is verified against. Constraints include anything the operator rules out of scope.

Open questions stay in the intent as written. They record what was unknown at approval, so they are not struck out once answered. Every one is answered before the builder hands off for acceptance, by a decision in the spec that names the question. "Out of scope here" and "deferred until X" are answers. If an answer changes the proposed outcome or the constraints, the operator changes those sections as well.

### `spec.md` - when there are decisions

What was decided and why. One numbered decision at a time, each carrying its own reasoning, so a later reader can overturn one without unpicking the rest.

Skip it when the work holds no decision worth recording. A spec written to make a pipeline look complete is waste.

The operator and the builder both take decisions. A decision the operator sets steers the implementation without prescribing the plan. The builder adds decisions as investigation turns them up. When the operator settles something in conversation, the builder writes it into the spec, so the operator does not have to maintain the file by hand.

A decision the builder takes carries the line `Proposed: awaiting operator approval.` under its heading, and the builder may revise it freely. When the operator approves it, in review or in conversation, the builder removes the line, so the commit records the approval. A decision the operator set is written without the line. A decision without the line is approved, and reversing or materially changing it goes back to the operator.

Questions the builder raises during the build are not added to the intent. When the question is how to do the work, and every plausible answer stays within the intent and the approved decisions, the builder answers it as its own decision. When it is about what the work is, the builder stops and asks the operator, and the answer becomes a decision.

The operator may also state preferences: approaches the operator leans towards but has not decided. List them under a `## Preferences` heading above the decisions. The builder may depart from a preference with good reason, and records the departure as a decision that names the preference and the evidence against it. A preference the builder must not depart from is a decision, and belongs with the decisions.

Every decision belongs to the pipeline that made it. There is no separate decision tree; a decision lives in exactly one spec. A later pipeline that overturns one says so with an `Overturns:` line under its own decision's heading, naming the pipeline number and the decision, such as `Overturns: 00001 D2`. Overturning another pipeline's decision goes to the operator.

Some decisions constrain work beyond their own pipeline. Mark those with a `Binding:` line naming who has to obey, directly under the heading:

    ### Dn — Decision title

    Binding: the later pipeline that must obey.

    A later pipeline is in violation if [concrete behavior].

    What would overturn this: [evidence that would change the decision].

The test for whether a decision is binding is concrete: name the pipeline that could violate it without noticing. If you cannot name one, it is not binding, and the line is left off. Most decisions are local to their own work.

A binding decision states plainly what counts as a violation, and carries a `What would overturn this` paragraph naming the evidence that would change the answer - not "if it stops working".

The binding set is derived, never maintained, and is listed in the order decisions appear:

```bash
awk '/^### /{h=substr($0,5)} /^Binding:/{print FILENAME"\t"h} /^Overturns:/{print FILENAME"\t"h"\t"$0}' docs/pipelines/*/spec.md | sed 's|docs/pipelines/||; s|/spec.md||'
```

The list is for discovery. It includes every `Overturns:` line. It does not show which decisions are still proposed, so read each listed decision before applying it.

- A binding decision stops binding once a later decision that overturns it is approved. A proposed reversal leaves it in force.
- A proposed binding decision is followed as if approved until the operator decides. Obeying a constraint that is later dropped costs less than breaking one that is later approved.

### `plan.md` - when the build is more than a couple of steps

The implementation steps, and what has actually been verified against what is only asserted. The builder writes it after inspecting the repository, and revises it whenever the build teaches something the plan did not know. At the end the plan and the diff agree; if they disagree, the plan was wrong and gets corrected.

## Status

`intent.md` opens with YAML frontmatter holding a single field. Nothing else tracks state.

```yaml
---
status: in-progress
---
```

Frontmatter holds document metadata, so only fields describing the pipeline as a whole belong there. `Binding:`, `Proposed:`, and `Overturns:` stay inline beside the decision they qualify, because each describes one decision rather than the spec that contains it. Lifting them into frontmatter would create a document-level list that restates what the body already says, and the two would drift.

| `status`      | Meaning                                         |
| ------------- | ----------------------------------------------- |
| `draft`       | the intent is still being written               |
| `approved`    | the operator approved it; work may start        |
| `in-progress` | being built                                     |
| `done`        | shipped, and the outcome verified               |
| `abandoned`   | closed without shipping; the intent records why |

The operator moves `draft` to `approved`, and confirms `done` and `abandoned`. The builder moves `approved` to `in-progress`, and says so in the handoff. The `status` field describes the pipeline, not the problem, so moving it does not change the intent. A status left unmoved is a wrong index, since both status commands read only this field.

There is no index file. The index is derived:

```bash
grep -m1 -H '^status:' docs/pipelines/*/intent.md | sed 's|docs/pipelines/||; s|/intent.md:status: |  |' | sort
```

The question asked most often has its own line:

```bash
grep -l '^status: in-progress' docs/pipelines/*/intent.md | sort
```

A maintained index is a second copy of a fact that already exists in the intents.

Both status commands sort explicitly because some `grep` implementations search files in parallel and return them out of order. A plain glob is already sorted, so on a stock `grep` the sort changes nothing.

## Building a pipeline

The goal of a pipeline is the outcome in its intent, not its plan. The builder's job is to make the proposed outcome true, within the intent's constraints and the spec's approved decisions. The plan is the current route; the intent is the completion condition. When an agent is given a goal, the goal is the intent's outcome, not finishing `plan.md`.

The builder works in a loop:

1. Read the intent, the spec if there is one, and the binding decisions from other pipelines.
2. Inspect the repository.
3. Write or revise the plan.
4. Build.
5. Verify the result against the intent's proposed outcome.
6. If the outcome is not yet true, go back to step 2 with what was learned. If it is true, hand off for the operator to accept.

The loop revises the plan, never the intent.

## Closing a pipeline

A pipeline is `done` when the outcome stated in its intent is true and verified. It is not done because the code merged or because the plan ran out of steps. Nor is it done while any decision in its spec is still proposed. The builder verifies, and the operator confirms. The plan records what was verified and by which command, or the handoff does when there is no plan. Anything deliberately deferred is named in the spec together with the condition that would bring it back. Deferring part of the proposed outcome changes the intent, so it goes to the operator.

Findings from operating the system open a new pipeline. Closed pipelines stay closed.

## What we took from the playbook

- **The intent as a versioned artifact** that states the problem separately from the solution. This is what agents lose fastest and what costs the most to reconstruct.
- **Artifacts committed to version control**, so the commit history is the audit trail of what was asked, what was produced, and what was approved.
- **Explicit approval gates** rather than remembered ones. The playbook records approval as the merge of the intent; the `status` field is an addition here, so a draft can sit in the tree before it is approved.
- **Operational findings re-entering as new intents.**

## What we dropped, and why

- **Separate originator, product owner, engineer, and release manager roles.** There is one operator. Role-based gates collapse into a single approval gate held by the operator.
- **A spec and a plan by default.** Three documents before the first line of code is right for work carrying real risk and wrong for most work. The intent is required; the spec and plan are written when they earn it.
- **A requirements and design spec.** The playbook's spec restates the intent as requirements, then adds the design. Here the intent's proposed outcome and constraints are the requirements, and the spec holds only decisions. Restating the requirements would put a second copy of operator-owned content in a shared file.
- **A logged prompt and skill versions beside the spec.** The playbook generates the spec in one prompted session. Here the spec grows one decision at a time, and each decision carries its own reasoning, so there is no single prompt to log.
- **Proof in the plan.** The playbook's plan says how the work will be proved. Here the intent's proposed outcome says how to tell when it is true, and the plan records only how that was verified. A builder that owned the completion condition could redefine done to match what it built.
- **A risks section in the plan.** A risk that shapes the build becomes a decision in the spec, where it carries its reasoning and can be marked binding.
- **Design and Build as separate stages with separate approvals.** Here they are one conversation. The gates are on the intent going in and the verification coming out.
- **A shared `/intent/` folder holding intents apart from their work.** Splitting one unit of work across two trees makes a reader reassemble it. One directory holds the whole pipeline.
- **Autonomous maintenance loops that act on detected breaches.** An agent acting on its own detection needs a control band, and setting one takes operating data this process does not assume. Detection can come later; autonomous action is not adopted.

<!-- /devloop:pipelines -->
