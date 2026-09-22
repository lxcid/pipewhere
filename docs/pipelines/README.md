# Pipelines

A pipeline is one unit of work, from the problem that motivated it to the code that closed it. Everything about that work lives in one directory.

    docs/pipelines/00001-infrastructure/
      intent.md    the problem, in the operator's terms
      spec.md      the decisions taken
      plan.md      the implementation steps

The shape is adapted from [Anthropic's AI-native SDLC playbook](https://claude.com/blog/the-ai-native-sdlc-playbook). We kept the parts that survive a team of one plus agents and dropped the rest. What we dropped is recorded at the bottom, so the decision does not get relitigated every time someone reads the original playbook.

## Naming

    docs/pipelines/<NNNNN>-<slug>/

Five digits, zero-padded, starting at `00001`. Numbers are assigned when a pipeline is opened and are never reused, including for abandoned pipelines. A number is an identifier. It is not a priority and not a build order.

The slug is lowercase and hyphenated, and it names the work rather than the solution.

## The three files

### `intent.md` - always

The problem, in the operator's words, written before a solution exists. It is not edited afterwards to match what was built. An intent rewritten to describe its own implementation has lost the only thing it was for.

An agent may draft it. The operator approves it. Approval is the gate: no spec, no plan, and no code until the frontmatter says `approved`.

Frontmatter carries `status` and nothing else. Sections: Problem, Proposed outcome, Affected users and systems, Constraints, Open questions.

### `spec.md` - when there are decisions

What was decided and why. One numbered decision at a time, each carrying its own reasoning, so a later reader can overturn one without unpicking the rest.

Skip it when the work holds no decision worth recording. A spec written to make a pipeline look complete is waste.

Every decision belongs to the pipeline that made it. There is no separate decision tree; a decision lives in exactly one spec, and a later pipeline that overturns one says so and links back.

Some decisions constrain work beyond their own pipeline. Mark those with a `Binding:` line naming who has to obey, directly under the heading:

    ### D4 - Postgres is the source of truth; Restate holds execution state only

    Binding: every pipeline that adds a workflow.

    A later pipeline is in violation if reading business state requires querying Restate, or if a workflow writes a row the application layer did not.

    What would overturn this: a read path where the Postgres round-trip is measurably too slow and Restate's keyed state is the natural place for a hot copy.

The test for whether a decision is binding is concrete: name the pipeline that could violate it without noticing. If you cannot name one, it is not binding, and the line is left off. Most decisions are local to their own work.

A binding decision states plainly what counts as a violation, and carries a `What would overturn this` paragraph naming the evidence that would change the answer - not "if it stops working".

The binding set is derived, never maintained, and is listed in the order decisions appear:

```bash
awk '/^### /{h=substr($0,5)} /^Binding:/{print FILENAME"\t"h}' docs/pipelines/*/spec.md | sed 's|docs/pipelines/||; s|/spec.md||'
```

### `plan.md` - when the build is more than a couple of steps

The implementation steps, and what has actually been verified against what is only asserted. The plan is kept current as the build proceeds. At the end the plan and the diff agree; if they disagree, the plan was wrong and gets corrected.

## Status

`intent.md` opens with YAML frontmatter holding a single field. Nothing else tracks state.

```yaml
---
status: in-progress
---
```

Frontmatter holds document metadata, so only fields describing the pipeline as a whole belong there. `Binding:` stays inline beside the decision it qualifies, because it describes one decision rather than the spec that contains it. Lifting it into frontmatter would create a document-level list that restates what the body already says, and the two would drift.

| `status`      | Meaning                                         |
| ------------- | ----------------------------------------------- |
| `draft`       | the intent is still being written               |
| `approved`    | the operator approved it; work may start        |
| `in-progress` | being built                                     |
| `done`        | shipped, and the outcome verified               |
| `abandoned`   | closed without shipping; the intent records why |

The operator moves `draft` to `approved`, and confirms `done` and `abandoned`. Whoever is building moves `approved` to `in-progress`, and says so in the handoff. A status left unmoved is a wrong index, since both status commands read only this field.

There is no index file. The index is derived:

```bash
grep -m1 -H '^status:' docs/pipelines/*/intent.md | sed 's|docs/pipelines/||; s|/intent.md:status: |  |' | sort
```

The question asked most often has its own line:

```bash
grep -l '^status: in-progress' docs/pipelines/*/intent.md
```

A maintained index is a second copy of a fact that already exists in the intents.

The status index sorts explicitly because some `grep` implementations search files in parallel and return them out of order. A plain glob is already sorted, so on a stock `grep` the sort changes nothing.

## Closing a pipeline

A pipeline is `done` when the outcome stated in its intent is true and verified, not when the code merged. The plan records what was verified and by which command. Anything deliberately deferred is named in the spec together with the condition that would bring it back.

Findings from operating the system open a new pipeline. Closed pipelines stay closed.

## What we took from the playbook

- **The intent as a versioned artifact** that states the problem separately from the solution. This is what agents lose fastest and what costs the most to reconstruct.
- **Artifacts committed to version control**, so the commit history is the audit trail of what was asked, what was produced, and what was approved.
- **Explicit approval gates** rather than remembered ones. The playbook records approval as the merge of the intent; the `status` field is this repository's addition, so a draft can sit in the tree before it is approved.
- **Operational findings re-entering as new intents.**

## What we dropped, and why

- **Separate originator, product owner, engineer, and release manager roles.** There is one human. Role-based gates collapse into a single approval gate held by the operator.
- **A spec and a plan by default.** Three documents before the first line of code is right for work carrying real risk and wrong for most work. The intent is required; the spec and plan are written when they earn it.
- **Design and Build as separate stages with separate approvals.** Here they are one conversation. The gates are on the intent going in and the verification coming out.
- **A shared `/intent/` folder holding intents apart from their work.** Splitting one unit of work across two trees makes a reader reassemble it. One directory holds the whole pipeline.
- **Autonomous maintenance loops that act on detected breaches.** Nothing is in production, and an agent acting on its own detection needs a control band we have no data to set. Detection can come later; autonomous action is not adopted.
