# `command-slice` skill — design

**Date:** 2026-07-04
**Status:** approved design, pending spec review
**Deliverable:** a project skill at `.claude/skills/command-slice/` that orchestrates
the end-to-end implementation of one NextExpress command slice, from assessing what to
build next through to a verified behaviour match against the live AmiExpress reference.

---

## 1. Purpose

Shipping a NextExpress command slice today is a fixed, high-discipline ritual:
assess the roadmap → capture the real AmiExpress wire behaviour → design the
adaptation against the Allium specs → build it test-first with mutation gating →
prove parity against the live reference. The steps, the artefacts, and the hazards
(FS-UAE node-spin, door-shadowed commands, capture blind spots, mutation gaps) are
all known and written down across `AGENTS.md`, `SLICES.md`, and the auto-memory.

This skill encodes that ritual as a repeatable, model-pinned, subagent-driven
pipeline so a single `/command-slice` invocation carries a slice from "what's next"
to "merged, with parity proven" — pausing only at the decision points where a human
should steer.

**Non-goals.** This skill does not replace the Allium specs, `SLICES.md`, or the
comparison harness — it *drives* them. It is not a general refactoring or debugging
tool. It targets the specific shape of work described in `AGENTS.md` → *Key Workflow*.

---

## 2. Placement, name, invocation

- **Location:** `.claude/skills/command-slice/` — a **project** skill, committed to the
  repo so it travels with `SLICES.md`, `designs/`, `specs/`, and the comparison
  harness it references.
- **Name:** `command-slice` (matches the repo's "slice" vocabulary).
- **Invocation:** `/command-slice` to let Stage 1 pick the next command, or
  `/command-slice <token>` (e.g. `/command-slice FR`) to target a specific command.

---

## 3. Architecture

The skill is a **process-playbook**, not a single deterministic `Workflow`. The main
conversation orchestrates six stages and **pauses at human gates** (plan, design,
comparison, discrepancy); a single top-to-bottom `Workflow` cannot pause for a human
mid-run.

**Reconciling rigour workflows with the gates.** Heavy stages run a `Workflow`
*inside the stage*: the workflow fans out, judges, and adversarially verifies, then
returns **one synthesized artefact**, and *then* the stage's human gate fires. So
"workflows for rigour" and "pause for me" compose cleanly — the playbook is the outer
loop; workflows are the inner engines of the thinking-heavy stages.

Embedding workflows this way also unlocks two things plain Agent-dispatch cannot:
**per-agent effort tiers** (`xhigh`/`max` on the hardest judge/refute passes) and
**structured judge-panel / adversarial patterns**.

### Model map

| Stage | Work | Model(s) | Parallelism |
|---|---|---|---|
| 1 Assess & plan | progress, next command, pre-refactors, slice plan | **Opus** | light fan-out of readers → synthesize + prereq critic |
| 2 Capture truth | drive live FS-UAE, document user-visible experience | **Opus** | **serial** on the board + completeness critic (re-probe loop) |
| 3 Design | adapt design, conform to Allium, drive captured behaviour | **Fable** designers, **Opus** judges/critics | **full workflow**: judge-panel + adversarial refutation |
| 4 Build | TDD execute, mutate, iterate | **Fable** implementer, **Opus** reviewer | sequential build + post-build verification workflow |
| 5 Compare | scenario → 2 testers → cross-mark | **Opus** (all) | **full workflow**: multi-scenario pipeline, board-serialized |
| 6 Resolve | discrepancy triage → human decision → back to 3 | **Opus** | parallel root-cause triage per divergence |

Fable does the *generative/complex* work (design synthesis, implementation); Opus
does the *assessment/verification* work (planning, capture, judging, comparison,
triage). This is the user's "Fable for the complex, Opus for the rest" principle
applied at fine grain.

**Rigour depth: Targeted.** Full rigour workflows on Stages 3 and 5; lighter workflow
passes on 1, 2, 4, 6. (Not "maximal" — no full judge-panel on every stage.)

---

## 4. The six stages

Each stage lists: model, what it does, the workflow pattern (if any), the artefacts it
writes, and whether it ends at a human gate.

### Stage 1 — Assess & plan · Opus · **GATE**

Determine the next command and how to prepare for it.

- **Reads:** `SLICES.md` + the relevant `slices/cmds-*.md`, recent git, the legacy
  dispatch table (`amiexpress/express.e:28285`), and the Allium specs (`specs/*.allium`).
- **Light workflow:** fan out Opus readers (roadmap+git state / target-module
  refactor-scan / Allium-drift) → synthesize → a **prereq critic** asks "what
  dependency or pre-refactor did we miss?".
- **Writes the plan into the existing slice docs:** the In/Out-scope entry in
  `slices/<family>.md` and the roadmap row in `SLICES.md`. Names any pre-refactor to
  apply first.
- **Gate:** present the chosen command + plan + pre-refactors; wait for approval.

### Stage 2 — Capture truth · Opus · (no gate; feeds Stage 3)

Ground the user-facing behaviour in the live reference **before** any design.

- Uses the running FS-UAE board (booted at startup, concurrently with Stage 1;
  see §5). Drives it with a new
  `comparison/harness/<cmd>.py`, saving `comparison/transcripts/<cmd>.txt`.
- Writes a human-readable experience note under `comparison/evidence-<slice>/`
  (the `live-observations.md` shape) describing command → on-screen experience.
- **Edits the slice doc to reference these captures** so "command → experience" is an
  explicit record, not folklore.
- **Completeness critic (loop):** an Opus critic reviews the transcript — "does this
  cover every sub-behaviour and edge case? any mojibake / line-terminator smell?" —
  and requests re-probes until covered. This is the direct antidote to the documented
  capture blind spots ([tierd-oracle-defect-lessons]).
- Ends its board session with a clean `G Y` logoff (node-spin hazard).

### Stage 3 — Design · Fable + Opus · **full workflow** · **GATE**

Decide how to adapt the system for the command, rigorously.

- **Judge-panel:** 2–3 **Fable** designers, each with a distinct framing
  (minimal-change / cleanest-seam / closest-to-legacy), produce candidate designs.
  **Opus** judges score each on {Allium conformance, capture-parity with Stage 2,
  hexagonal-architecture cleanliness, test-first feasibility, blast radius}.
  Synthesize the winner, grafting the best ideas from runners-up.
- **Adversarial refutation:** Opus/Fable skeptics try to prove the winning design
  *violates* the Allium spec or *misses* a captured behaviour row — especially edge
  cases, where this project's source-derived guesses have historically been wrong.
- **Writes** `designs/YYYY-MM-DD-<cmd>-design.md`: what changes, how it conforms to
  `specs/*.allium`, how it drives the Stage-2 captured behaviour, plus an
  implementation plan.
- **Gate:** present the design + plan; wait for approval.

### Stage 4 — Build · Fable + Opus · sequential build + verification workflow

Execute the plan test-first.

- **Fable** implements the TDD loop: a failing test pinned to the Stage-2 capture
  literals → minimum code → `cargo nextest run` → `make mutants-diff` → refactor.
  Honours the wire-UTF-8 gate, hexagonal seams, and doc-comment style from `AGENTS.md`.
- Updates `COMMAND_PARITY.md` (including PLAUSIBLE rows for uncaptured edges) and
  `SYSTEM.md`.
- **Post-build verification workflow:** any surviving `make mutants-diff` mutant →
  an adversarial "find the untested behaviour" pass that adds/strengthens tests; plus
  an **Opus** capture-parity + Allium-drift review before "done".
- **Flags any unforeseen blocker back to the user** rather than guessing.

### Stage 5 — Compare · Opus ×N · **full workflow** · **GATE**

Prove behaviour and experience match, double-blind.

- **Scenario author** (Opus) emits a **set** of target-agnostic user-behaviour
  scenarios (happy path + the quarantined edge rows), each described purely as user
  intent/inputs (login → join area → list files → …).
- **`pipeline` per scenario:** Tester-A runs it against **NextExpress/telnet**;
  Tester-B runs the same against **live FS-UAE/telnet**. Each writes a session log:
  (a) scenario/inputs, (b) target, (c) per-step input → what they "saw".
- **Double-blind cross-mark:** each log is marked by an agent holding the *other's*
  log, flagging inconsistencies. A **completeness critic** asks "what did *both*
  testers fail to exercise?" (the "nothing missed" goal).
- Synthesize divergences into a comparison report under `comparison/evidence-<slice>/`.
- **Gate:** present the comparison result.

**Board-concurrency constraint (correctness):** the FS-UAE board is a singleton with
hazards (same-user two-node block, phantom login, node-spin on unclean close). The
reference-side testers (Tester-B) **must not fan out freely** — reference-side scenario
runs are **serialized** through one controlled board session (clean `G Y` each time),
while the cheap NextExpress side (Tester-A) and all cross-marking run in parallel. The
Stage-5 workflow encodes this so a future run never spins up colliding board logins.

### Stage 6 — Resolve · Opus · **GATE**

Close the loop on any divergence.

- For each divergence, a parallel **root-cause triage** agent (systematic-debugging
  posture) classifies it — NextExpress bug? capture artefact? spec ambiguity? intended
  departure? — and proposes a fix.
- **Presents the options to the user and asks how to resolve.** A resolution that
  needs code loops back to **Stage 3** with an updated plan. A clean comparison
  proceeds to teardown + merge (§5).

---

## 5. Board, port, and worktree lifecycle

### Worktree (per run)

- **Check before creating:** if a slice is already underway (an existing slice branch
  / worktree, or in-progress uncommitted slice work), **offer to resume it** — do not
  blindly create a new worktree.
- Otherwise create a fresh worktree off **`origin/main`** and do all stage work there.
- **At the end, merge to `main` directly — no PR.** The skill's changes are additive
  (new files + doc appends); the merge checks for conflicts against any concurrent
  session before completing, and warns rather than forcing.

### FS-UAE board + ports (per run)

- **Clean-state check first:** detect any already-running board container; **never
  kill or reuse another session's board** (it would corrupt that session's state).
  Boot **our own** container with a per-run name + per-run host port.
- **Per-run ports** are chosen by a Python helper (`resources/allocate_ports.py`,
  proven to run before ship): a free host port mapped to the board's telnet
  (`127.0.0.1:PORT→6023`) and a free port for the NextExpress server. This lets
  parallel runs (different worktrees) coexist.
- **Documented hazard:** parallel FS-UAE boards sharing the same `nextexpress-bbs`
  volume corrupt each other (`acpConnections.dat`, flag files). The safe default is a
  single board per run with serialized reference-side access; truly-parallel runs need
  cloned volumes. The skill states this and defaults safe.
- **Keep-alive:** boot once, early (during Stage 1, to hide the 2–3 min boot behind
  the assessment), reuse across Stages 2 and 5, and **tear down at the end** with a
  clean `G Y` logoff then `docker rm`.

All board-driving detail (login flow, DoS-ban gotcha, pager hazards, door-shadow
caveat) is distilled into `resources/board-lifecycle.md` from the
[amiexpress-docker-harness] memory.

---

## 6. Skill file structure

```
.claude/skills/command-slice/
  SKILL.md                     # lean playbook: 6 stages, gates, model map, invariants
  resources/
    board-lifecycle.md         # boot/keep/teardown FS-UAE, clean-state, G Y hazard, ports
    subagent-briefs.md         # exact per-stage prompts (model + role) to dispatch
    stage3-design.md           # design judge-panel + adversarial-refutation workflow shape
    stage5-comparison.md       # double-blind tester + cross-mark protocol + log templates
    artifact-conventions.md    # where each stage writes (slices/designs/comparison/PARITY/SYSTEM)
    allocate_ports.py          # per-run free-port picker + board-collision check
```

`SKILL.md` stays lean and always-loaded; the `resources/` files are loaded on demand
by the stage that needs them (the pattern used by the `hexagonal-architecture` skill).

---

## 7. AGENTS.md changes

The skill takes ownership of the live-verification responsibility, so `AGENTS.md` is
updated to match:

- **Add** — at the top of *Key Workflow*: "When implementing a new command/slice,
  **prefer the `command-slice` skill** (`.claude/skills/command-slice/`). It
  orchestrates assess → capture → design → build → compare with model-pinned
  subagents and drives the live boards for you."
- **Rewrite item 6** (*Before Committing*): keep the *reason* — scripted byte-equality
  has blind spots (pager echo, line terminators, on-screen rendering) — but reassign
  the mitigation from "a human has to look at the real terminal" to "the skill's agents
  drive the live NextExpress server, and **Stage 5's double-blind cross-mark against
  both live boards is the parity guarantee**." Record the residual limitation (no human
  observes true per-keystroke echo) as an accepted, documented tradeoff.
- **Rewrite item 7:** keep its substance (verify against live FS-UAE *while building*,
  capture transcripts, pin literals, clean `G Y`, door-shadow caveat) but attribute it
  to the skill's agent-driven Stage 2 rather than implying a human performs it.

### Overridden-guarantee tradeoff (honest record)

Fully agent-driven board verification **forfeits the human-eyes guarantee** that
`AGENTS.md` items 6 & 7 existed to enforce (the 2026-06-11 root-cause analysis;
[tierd-oracle-defect-lessons]). Stage 5's double-blind cross-marking — two independent
Opus agents observing *both* live boards and marking each other — is a strong
mitigation that catches rendering/echo divergence a single byte-diff would miss, but it
is **not equivalent** to a human at a real terminal for true per-keystroke echo. The
skill states this limitation at run time so the operator re-consents to it each run.

---

## 8. Success criteria

A run is successful when:

1. The next command was chosen with the plan recorded in `slices/` + `SLICES.md`.
2. The reference behaviour is captured (`comparison/transcripts/`,
   `comparison/evidence-<slice>/`) and referenced from the slice doc.
3. A design doc under `designs/` conforms to the Allium spec and drives the captured
   behaviour, approved at the gate.
4. The code is implemented test-first, `cargo nextest run` passes, `cargo build` is
   warning-free, `make mutants-diff` shows no un-addressed survivors, and
   `COMMAND_PARITY.md` + `SYSTEM.md` are updated.
5. Stage 5's double-blind comparison shows no un-triaged divergence between NextExpress
   and the live reference.
6. The branch is merged to `main`, and the board container is torn down cleanly.

---

## 9. Building the skill (this session)

1. Work in the clean worktree `nextexpress-command-slice` (branch `skill/command-slice`,
   off `origin/main`) — already created, isolating from the concurrent session.
2. Write this design doc (done), self-review, and get user review.
3. `superpowers:writing-plans` → implementation plan for authoring the skill files.
4. `superpowers:writing-skills` → author `SKILL.md` + `resources/`, prove
   `allocate_ports.py` runs, and apply the `AGENTS.md` edits.
5. Adversarial review of the drafted `SKILL.md` against `AGENTS.md` + repo conventions.
6. Commit and **merge to `main`** (no PR), then remove the worktree.
