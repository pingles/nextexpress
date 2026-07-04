# `command-slice` skill — design

**Date:** 2026-07-04
**Status:** v2 — hardened per the 2026-07-04 design audit; pending spec re-review
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

> **Every stage is additionally bound by the hardening requirements in §10** (derived
> from the 2026-07-04 correction audit). Where a stage says "→ §10.x", that rule is
> mandatory, not advisory.

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
- **Routes the slice (§10.5):** a user-facing command runs all six stages; a
  **non-user-facing slice** (pure refactor / port / infra, no wire surface) and any
  named pre-refactor take the **refactor track** that skips Stages 2 & 5. Stage 1
  declares which track applies — no empty capture theatre.
- **Gate:** present the chosen command + plan + pre-refactors + track; wait for approval.

### Stage 2 — Capture truth · Opus · (no gate; feeds Stage 3)

Ground the user-facing behaviour in the live reference **before** any design.

- Uses the running FS-UAE board (booted at startup, concurrently with Stage 1;
  see §5). Drives it with a new
  `comparison/harness/<cmd>.py`, saving `comparison/transcripts/<cmd>.txt`.
- Writes a human-readable experience note under `comparison/evidence-<slice>/`
  (the `live-observations.md` shape) describing command → on-screen experience.
- **Edits the slice doc to reference these captures** so "command → experience" is an
  explicit record, not folklore.
- **Completeness critic (bounded loop, §10.2):** an Opus critic checks the transcript
  against the **canonical edge-probe battery** (empty/junk input, out-of-range + non-
  numeric args, unknown token, trailing junk, bare-CR/bare-LF, lone-key vs line-read,
  each sub-prompt's accept/reject set, empty-collection gate paths, mojibake / line-
  terminator smell) — a capture row per applicable item. It is **capped** (§10.8): a
  sub-behaviour that is structurally uncapturable (timeout, door-pager consumption,
  two-node block) is resolved from `express.e` control-flow and tagged *extrapolated*,
  not looped on forever.
- **Tags each captured field** stable-const vs volatile-runtime (§10.6) and **flags
  interactive / pager / hotkey behaviour** for the Stage-5 human-glance prompt (§10.7).
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
  cases, where this project's source-derived guesses have historically been wrong. It
  also enforces the no-premature-abstraction rule (§10.4): a new port/adapter enum arm
  must cite a Stage-2 behaviour unreachable with existing seams.
- **Authority reconciliation (§10.3):** for a door-shadowed token, diff the AquaScan
  capture against the `express.e` dispatch/control-flow; **any divergence HALTS and is
  presented at the gate as an explicit A/B decision** (express.e-wins default), tagged
  per facet (bytes: capture / control-flow: source) and recorded in the design doc +
  `COMMAND_PARITY.md`. Never auto-resolved by a subagent.
- **Grammar table (§10.4):** the design enumerates every input form (bare, inline-arg,
  each sub-prompt + verb set) with its capture reference and intended handling.
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
- **Mutation-gate integrity (§10.1):** before `make mutants-diff`, `git add -N` every
  new untracked file and run crate-relative; **an implausibly-low mutant count fails the
  stage** (a green gate on 3-of-33 mutants is the silent trap). Test literals are
  independent (no self-referential pins); stable-const fields byte-pinned, volatile
  fields asserted by derivation (§10.6); every wire literal carries `express.e:N`
  provenance and port/store failures get a modelled-error test — no `unwrap/panic!` on
  port results (§10.4). Test files follow the sibling-`tests.rs` convention (§10.4).
- **Post-build verification workflow:** any surviving mutant → an adversarial "find the
  untested behaviour" pass that adds/strengthens tests; plus an **Opus** capture-parity
  + Allium-drift + existing-doc-staleness review (§10.4) before "done".
- **Flags any unforeseen blocker back to the user** rather than guessing.

### Stage 5 — Compare · Opus ×N · **full workflow** · **GATE**

Prove behaviour and experience match, double-blind.

- **Scenario author** (Opus) emits a **set** of target-agnostic user-behaviour
  scenarios (happy path + the quarantined edge rows), each described purely as user
  intent/inputs (login → join area → list files → …).
- **`pipeline` per scenario** (one scenario per grammar-table row, §10.4): Tester-A runs
  it against **NextExpress/telnet**; Tester-B runs the same against **live FS-UAE/telnet**.
  Each writes a session log: (a) scenario/inputs, (b) target, (c) per-step input → what
  they "saw". Tester-A drives a **character-at-a-time** interactive client recording
  bytes-after-each-keystroke (echo/no-echo, CR vs CRLF), not line-granular I/O (§10.7).
- **Interactive-slice human glance (§10.7):** if Stage 2 flagged interactive / pager /
  hotkey behaviour, the skill **pauses and asks the operator** whether to do a hands-on
  terminal glance before pinning — the one residual the double-blind agents cannot see.
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
  departure? — and proposes a fix. **Suspected reference ambiguity is confirmed, not
  assumed** (§10.2, §10.8): re-run the reference scenario within the connection budget to
  tell stable from noisy; if noisy or uncapturable, resolve the facet from `express.e` as
  tiebreak, tag it *extrapolated*, and surface it — never auto-blame NextExpress code.
- **Presents the options to the user and asks how to resolve.** A resolution that
  needs code loops back to **Stage 3** with an updated plan. A clean comparison
  proceeds to teardown + merge (§5).

---

## 5. Worktree, board, and server lifecycle

### Worktree (per run)

- **Check before creating:** if a slice is already underway (an existing slice branch
  / worktree, or in-progress uncommitted slice work), **offer to resume it** — do not
  blindly create a new worktree.
- Otherwise create a fresh worktree off **`origin/main`** and do all stage work there.
- **At the end, merge to `main` directly — no PR (§10.9).** Rebase on `origin/main`
  first; **never move a `main` that is checked out in another worktree**; treat the
  shared docs (`COMMAND_PARITY`/`SYSTEM`/`SLICES`/`AGENTS`) as merge-prone (append at
  stable anchors, re-audit after rebase) — the "additive edits never collide" assumption
  is false for exactly those high-contention files.

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

### NextExpress server + run-state (per run)

- **Symmetric server lifecycle (§10.5):** the NextExpress server gets the same treatment
  as the board — booted on the allocated port with a readiness/health check before Stage
  5, registered for teardown, and **killed by recorded PID on run-end and on any stage
  failure**. `allocate_ports.py` also detects a stale server squatting a candidate port.
- **Connection budget (§10.8):** a per-run cap of **< 5 telnet opens before recycling the
  container** so the board's DoS self-ban cannot strand a Stage-5 run mid-comparison.
- **Run-state + resume (§10.5):** a run-state file (stage, scenario index, ports,
  container name, server PID) is updated at each stage boundary. On resume, **reconcile
  live resources first** — drain and `G Y` any open board session, kill any stale server
  — *then* continue from the recorded stage rather than double-booting a second board
  (two-node block / DoS-ban).

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
    edge-probe-battery.md      # canonical edge-input checklist Stage 2 must capture (§10.2)
    hardening.md               # the §10 invariants as an operator checklist
    artifact-conventions.md    # where each stage writes + string/encoding/volatile-field rules
    allocate_ports.py          # per-run free-port picker; board + stale-server collision check
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
  observes true per-keystroke echo) as an accepted, documented tradeoff — **except that
  for interactive / pager / hotkey slices the skill still prompts for an optional human
  glance** (§10.7), since the double-blind agents cannot observe local per-keystroke echo
  any better than a capture can.
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
   warning-free, `make mutants-diff` (run with new files `git add -N`'d, crate-relative,
   and a **plausible mutant count for the diff** — §10.1) shows no un-addressed
   survivors, and `COMMAND_PARITY.md` + `SYSTEM.md` are updated with existing claims
   re-audited, not just appended (§10.4).
5. Stage 5's double-blind comparison shows no un-triaged divergence between NextExpress
   and the live reference.
6. The branch is merged to `main` (§10.9), and **both** the board container and the
   NextExpress server are torn down cleanly (clean `G Y`, killed by PID).

---

## 9. Building the skill (this session)

1. Work in the clean worktree `nextexpress-command-slice` (branch `skill/command-slice`,
   off `origin/main`) — already created, isolating from the concurrent session.
2. Write this design doc (done), self-review, and get user review.
3. `superpowers:writing-plans` → implementation plan for authoring the skill files.
4. `superpowers:writing-skills` → author `SKILL.md` + `resources/` (including
   `hardening.md` + `edge-probe-battery.md` encoding §10), prove `allocate_ports.py`
   runs, and apply the `AGENTS.md` edits.
5. Adversarial review of the drafted `SKILL.md` against `AGENTS.md` + repo conventions.
6. Commit and **merge to `main`** (no PR), then remove the worktree.

---

## 10. Hardening requirements (from the 2026-07-04 correction audit)

Each item is a recurring, documented correction this project has already paid for
(5-source mining + coverage assessment + adversarial critique). They are **mandatory**
and are referenced by the stages in §4. Grouped, not ranked; severity noted. Distilled
into `resources/hardening.md` as an operator checklist.

**§10.1 — Mutation-gate integrity (HIGH).** `make mutants-diff` silently under-reports:
repo-relative paths log "No mutants to filter", and new *untracked* files are invisible
to `git diff HEAD` (a fresh `commands.rs` shows 3 mutants, not 33) — a green gate over
untested code. Stage 4 must: `git add -N` every new file before mutating; run
crate-relative (`git diff HEAD --relative` from `rust/`); and **fail the stage if the
mutant count is implausibly low for the diff size**. A low count is a failure, not a
pass. (Cost: [cargo-mutants-in-diff-paths]; untracked miss observed 2026-07-03. Every
slice creates new files, so this trips every run.)

**§10.2 — Capture the edges; fall back to source when an edge is uncapturable (HIGH).**
The single most frequent failure is edge behaviour guessed from a happy-path capture or
a source read, then refuted (N multi-conf / CF-gating; "AE is silent" claims;
`** AutoSaving **` unconditional; D9 date-prompt edges). Stage 2's completeness critic
must mechanically satisfy the **edge-probe battery** (`resources/edge-probe-battery.md`:
empty/whitespace, out-of-range + non-numeric args, unknown token, trailing junk after a
valid arg, bare-CR + bare-LF, lone-key vs line-read, every sub-prompt's accept/reject
set, empty-collection gate paths) — a capture row per applicable item before it can
pass. When an item is **structurally uncapturable** (timeout, door-pager eats the
command, single-user two-node block), stop re-probing and resolve it from `express.e`
control-flow (`displayFileList:27626`, `getDirSpan:26857`), tagged *extrapolated-from-
source* in `COMMAND_PARITY.md` — never guessed from the partial bytes.

**§10.3 — Door/source authority is a human decision, surfaced not auto-resolved (HIGH).**
For door-shadowed tokens (F/FR/N/SCAN/NSU/CS/SENT) the AquaScan door and `express.e`
genuinely conflict, and which wins is the operator's call (bare `FR` shipped to the
capture, reversed two days later — commit `fa2a855`, 9 files). Stage 3 diffs the two;
**any divergence HALTS and is presented at the gate as an A/B decision** (express.e-wins
default), recorded captured-vs-extrapolated in the design doc + `COMMAND_PARITY.md`.
Authority is tagged **per facet**: AquaScan owns wire bytes, `express.e` owns silent
control-flow (the NextScan rebrand mixes both within one token). No subagent
auto-resolves a door/source conflict. (See [use-original-amiexpress-code],
[tierd-aquascan-parity-target].)

**§10.4 — Provenance, grammar, and no-drift discipline (MEDIUM).** Cheap rules for the
Stage-4 brief + the post-build review:
- **Binding provenance:** every letter the slice binds cites its `express.e:28285`
  dispatch line or is a labelled departure; menu-asset rows diffed **verbatim** (no token
  pre-filter — that is how the advertise-then-reject drift survived).
- **String provenance:** every user-facing literal carries `express.e:N` or a labelled
  deliberate-departure note + `COMMAND_PARITY.md` row; review rejects unprovenanced wire
  strings.
- **Grammar table:** the Stage-3 design enumerates every input form (bare / inline-arg /
  each sub-prompt + verb set); the adversarial pass hunts for an accepted form with no
  row; Stage 5 runs one scenario per row (D7 patched D4 same-day over a dropped
  inline-arg; B10 reworked the whole `R` loop).
- **Failure-path:** any handler touching a port/store gets a failing-adapter test proving
  a *modelled error* and no partial commit on abort/EOF/idle; review grep-rejects
  `unwrap/expect/panic!` on port results (~7 "don't panic when save fails" commits).
- **No self-referential pins:** expected literals are independent bytes, never derived
  from the same const / `\`-continuation idiom they assert against (the D9 vacuous pin).
- **Doc-audit:** for every doc section the slice touches, re-audit existing claims
  against code / `express.e` (a stale "flip private default to Y" note nearly regressed
  parity) — not just append. Guard tests assert over full unfiltered content.
- **No premature abstraction:** a new port/adapter enum arm must cite a Stage-2 behaviour
  unreachable with existing seams; the adversarial pass tries to show it is reachable
  without (the `Silent` EchoMode variant, built then deleted — "you needed `read_key`").
- **Test placement:** large/test-dominated modules → sibling `tests.rs`; small → inline;
  never `foo_test.rs` / `#[path]` ([test-placement-convention]; auto-memory-only, so the
  brief must restate it).

**§10.5 — Both server and board have lifecycles; runs are resumable (MEDIUM).** The
NextExpress server gets a lifecycle **symmetric to the board**: boot on the allocated
port with a health check, register for teardown, kill by recorded PID on run-end and on
any stage failure; `allocate_ports.py` detects a stale server squatting a port. A
**run-state file** (stage, scenario index, ports, container name, server PID) is updated
at each stage boundary; **resume reconciles live resources first** (drain + `G Y` any
open board, kill stale server) before continuing — never double-books a board. The
**non-user-facing / refactor track**: a slice with no wire surface (and any Stage-1
pre-refactor) skips Stages 2 & 5 and gates on `nextest` + warning-free build +
`mutants-diff` (§10.1) + Allium-drift review — no capture theatre.

**§10.6 — Volatile vs stable literals (MEDIUM).** Stage 2 tags every captured field
stable-const (glyphs, prompts, dash geometry) vs volatile-runtime (dates, times,
node/conf numbers, last-call-derived defaults). Stage 4 byte-pins only stable-const;
volatile fields assert *format/derivation* (e.g. `== user.last_call()`-derived default,
`mm-dd-yy` shape), never the captured literal (the `06-25-26` date default drifts by
capture day).

**§10.7 — Encoding + the interactive-echo residual (MEDIUM).** Every captured byte ≥0x80
is recorded with **both** its Latin-1 byte and its target UTF-8 code point; Stage-4 test
literals use the `&str` code-point form; a `COMMAND_PARITY.md` encoding-departure row per
high-bit surface (naive verbatim paste = mojibake before the e2e gate exists). Tester-A
drives NextExpress **character-at-a-time**, logging bytes-after-each-keystroke. For any
slice whose Stage-2 note flags interactive / pager / hotkey behaviour, the skill
**prompts the operator for an optional hands-on terminal glance** before pinning — the
residual the double-blind agents cannot observe (the D2 dead-pager class only a human
caught). This is the agreed exception to the otherwise fully-agent-driven §7 tradeoff.

**§10.8 — Budgets, bounded loops, stall recovery (MEDIUM).** Every convergence loop is
capped: Stage-2 completeness re-probes, Stage-3 judge/refutation rounds, Stage-5
per-scenario pairs. A loop that hits its cap escalates to a human gate rather than
spinning. A per-run **telnet connection budget (< 5 opens before recycling the
container)** keeps the board's DoS self-ban from stranding a run; per-stage token/effort
budgets bound cost. Subagent stalls (completed-tool-then-silence = API outage) are
**resumed, not re-prompted**; any open board session is `G Y`'d before an attempt is
abandoned; there is no `timeout(1)` on this host — use the Monitor/until-loop pattern.
([workflow-cargo-stall-hazard].)

**§10.9 — Merge discipline (MEDIUM).** Merge to `main` with no PR, but: rebase on
`origin/main` first; **never move a `main` checked out in another worktree** (merge from
a quiescent vantage); and reconcile the high-contention shared docs
(`COMMAND_PARITY`/`SYSTEM`/`SLICES`/`AGENTS`) at stable anchors, re-auditing after rebase
— same-region appends still conflict.

**§10.10 — Gates present decisions; model failures escalate (MEDIUM).** Human gates
present **structured decisions**, not artefacts to rubber-stamp: any detected door/source
(§10.3), capture/source, or volatile-field ambiguity is shown as an explicit choice with
the express.e-wins default and recorded (captured vs extrapolated). A Fable
design/implementation that will not compile, or an irreconcilable Opus-judge-vs-Fable
disagreement, **escalates**: retry once → escalate the failing role Fable→Opus → halt to
a human gate. The least-bad candidate never ships silently, and no loop hides a model
failure as "converged".
