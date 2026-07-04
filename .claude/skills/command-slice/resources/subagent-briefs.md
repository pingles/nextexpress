# subagent-briefs.md

Ready-to-dispatch briefs for every subagent the `command-slice` pipeline spawns. Each
brief is a self-contained imperative prompt plus a `dispatch:` line (model + effort). Adapt
the bracketed `<…>` slots per run. Model map is authoritative from `SKILL.md`: Stage 1 Opus;
Stage 2 Opus; Stage 3 Fable designers + Opus judges/critics; Stage 4 Fable implementer + Opus
reviewer; Stage 5 all Opus; Stage 6 Opus. Effort tiers: `xhigh`/`max` are reserved for the
hardest judge/refute passes (Stage 3 refuter, Stage 5 cross-mark). Honor the §10 rules cited —
full text in `hardening.md`.

Cross-refs: workflow shapes in `stage3-design.md` and `stage5-comparison.md`; the edge checklist
in `edge-probe-battery.md`; write-locations and encoding rules in `artifact-conventions.md`;
board driving in `board-lifecycle.md`.

---

## Stage 1 — Assess & plan (Opus) · GATE

Light fan-out of three readers → synthesize → prereq critic. Synthesis is done by the
orchestrator; the three readers run in parallel.

### 1a · Roadmap + git-state reader

> Read `SLICES.md` and the relevant `slices/cmds-*.md`, plus the last ~15 commits
> (`git log --oneline -15`) and working-tree status. Determine what shipped most recently and
> what the roadmap says is next. If the invocation named a token `<TOKEN>`, locate its roadmap
> row and In/Out-scope entry instead of picking. Report: the candidate next command, its family
> doc, its roadmap row, and any in-progress/uncommitted slice work that means we should **resume**
> rather than start fresh. Do not design or capture — just establish state.

`dispatch: model=opus, effort=medium`

### 1b · Target-module refactor-scan reader

> For the candidate command `<TOKEN>`, read the module(s) that will host it and the legacy
> dispatch table `amiexpress/express.e:28285`. Identify any **pre-refactor** the slice needs
> before new behaviour lands (a seam to extract, a duplicated block to unify, a port that should
> exist first). Name each pre-refactor concretely with the file/symbol. Flag whether the slice
> itself is **user-facing** (adds/changes wire bytes) or **non-user-facing** (pure refactor/port/
> infra) — this decides the track (§10.5). Do not write code.

`dispatch: model=opus, effort=medium`

### 1c · Allium-drift reader

> Read the Allium specs in `specs/*.allium` that govern `<TOKEN>` (and its command family).
> Summarize the obligations the slice must satisfy and flag any place the current implementation
> has already **drifted** from the spec. Note obligations that the design (Stage 3) and tests
> (Stage 4) will have to honor. Cite spec section names. Frame it as: what tests and
> invariants does the spec demand for this command?

`dispatch: model=opus, effort=medium`

### 1d · Prereq critic

> Inputs: the synthesized Stage-1 plan (chosen command, named pre-refactors, declared track) and
> the three readers' notes. Ask one question hard: **what dependency or pre-refactor did we
> miss?** Check for an unnamed seam, an Allium obligation with no home in the plan, a token that
> is door-shadowed (F/FR/N/SCAN/NS/NSU/CS/SENT) and therefore carries a §10.3 authority decision,
> and whether the declared track (§10.5) is right — no capture theatre for a refactor, no skipped
> capture for a wire slice. Output a short PASS/AMEND verdict with concrete additions. This is a
> **bounded** pass (§10.8): one round, then the plan goes to the human gate.

`dispatch: model=opus, effort=high`

**Stage-1 output artefact:** the In/Out-scope entry appended to `slices/<family>.md` and the
roadmap row in `SLICES.md`, plus the declared track. §10 honored: **§10.5** (track routing),
**§10.3** (flag door-shadow authority decisions early), **§10.10** (gate presents a structured
decision, not a rubber-stamp).

---

## Stage 2 — Capture truth (Opus) · serial on the board · feeds Stage 3

Board already booted (Stage 1 startup). Serial single-session access; end every session with a
clean `G Y` logoff. Respect the connection budget (< 5 opens before recycling — §10.8).

### 2a · Capture driver

> Drive the live FS-UAE board over telnet to capture the real wire behaviour of `<TOKEN>`. Write
> a driver `comparison/harness/<cmd>.py` and save its transcript to
> `comparison/transcripts/<cmd>.txt`. Then write a human-readable experience note under
> `comparison/evidence-<slice>/` in the `live-observations.md` shape: command → on-screen
> experience, per prompt and sub-prompt. Edit the slice doc to reference these captures so
> "command → experience" is recorded, not folklore.
>
> Constraints:
> - **Door-shadow caveat (§10.3):** if `<TOKEN>` is F/FR/N/SCAN/NS/NSU/CS/SENT you are capturing
>   the AquaScan door, not the internal command — label it as such; the source facet is Stage 3's
>   job to reconcile.
> - **Encoding (§10.7):** record every captured byte ≥0x80 with **both** its Latin-1 byte and its
>   target UTF-8 code point. Never paste raw high-bit bytes.
> - **Volatile vs stable (§10.6):** tag every captured field stable-const (glyphs, prompts, dash
>   geometry) vs volatile-runtime (dates, times, node/conf numbers, last-call-derived defaults).
> - **Interactive flag (§10.7):** if the command shows pager / hotkey / per-keystroke behaviour,
>   flag it in the note for the Stage-5 human-glance prompt.
> - **Budget + hygiene (§10.8):** stay under the connection budget; clean `G Y` logoff at the end.

`dispatch: model=opus, effort=high`

### 2b · Completeness critic (edge-probe-battery gated)

> Inputs: the Stage-2 transcript + evidence note, and `resources/edge-probe-battery.md`. Check the
> capture mechanically against **every applicable** battery item: empty/whitespace input, out-of-
> range + non-numeric args, unknown token, trailing junk after a valid arg, bare-CR vs bare-LF,
> lone-key vs line-read, each sub-prompt's accept/reject set, empty-collection gate paths, and
> mojibake / line-terminator smell. Require a capture row per applicable item.
>
> **Bounded re-probe loop (§10.2, §10.8):** for each missing item, send the driver back for one
> more targeted probe. But if an item is **structurally uncapturable** (timeout, door-pager
> consumes the command, single-user two-node block), STOP re-probing: resolve it from `express.e`
> control-flow (e.g. `displayFileList:27626`, `getDirSpan:26857`), and tag the row
> *extrapolated-from-source* in `COMMAND_PARITY.md` — never guessed from partial bytes. Cap the
> loop; escalate to a human gate if it will not converge.

`dispatch: model=opus, effort=high`

**Stage-2 output artefact:** `comparison/harness/<cmd>.py`, `comparison/transcripts/<cmd>.txt`, an
`evidence-<slice>/live-observations.md` note with fields tagged stable/volatile and interactive
surfaces flagged, and edge rows (captured or extrapolated) ready for the grammar table. §10
honored: **§10.2**, **§10.6**, **§10.7**, **§10.8**.

---

## Stage 3 — Design (Fable designers + Opus judges/critics) · full workflow · GATE

Judge-panel + adversarial refutation. Detailed workflow shape in `stage3-design.md`.

### 3a · Fable designers (2–3, distinct framings)

Spawn one per framing. Each gets the same inputs, a different mandate.

> Inputs: the Stage-2 captures + evidence note (with tagged fields and grammar rows), the Allium
> obligations from Stage 1c, the target module, and `amiexpress/express.e:28285`. Produce a
> candidate design for `<TOKEN>` under your assigned framing:
> - **Framing A — minimal-change:** the smallest diff that satisfies the captures and the spec;
>   reuse existing seams; add nothing speculative.
> - **Framing B — cleanest-seam:** the most hexagonally-clean port/adapter decomposition, even at
>   a slightly larger diff; domain core stays free of non-domain deps.
> - **Framing C — closest-to-legacy:** the design that most faithfully mirrors `express.e`
>   control-flow and user-visible ordering.
>
> Every candidate must include a **grammar table (§10.4)** enumerating every input form (bare,
> inline-arg, each sub-prompt + verb set) with its Stage-2 capture reference and intended
> handling, and an implementation plan. Cite `express.e:N` for each bound letter/string. Do not
> introduce a new port/adapter enum arm unless you cite a Stage-2 behaviour unreachable with
> existing seams (§10.4).

`dispatch: model=fable, effort=high` (one dispatch per framing)

### 3b · Opus judge panel

> Inputs: the 2–3 candidate designs. Score each on {Allium conformance, capture-parity with
> Stage 2, hexagonal-architecture cleanliness, test-first feasibility, blast radius}. Produce a
> ranked table with a one-line justification per cell, then **synthesize a winner**, grafting the
> best ideas from the runners-up. Name explicitly what you took from each. Do not soften a low
> score to force a consensus (§10.10).

`dispatch: model=opus, effort=high`

### 3c · Adversarial refuter (Opus, escalate to Fable if stuck)

> Inputs: the synthesized winning design + all captures + the Allium spec. Try to **prove the
> design wrong**: that it violates an Allium obligation, or misses a captured behaviour row —
> especially edge cases, where this project's source-derived guesses have historically been
> refuted. Hunt specifically for: an accepted input form with no grammar-table row (§10.4); a
> new port/adapter enum arm that is actually reachable without it (§10.4, the deleted `Silent`
> EchoMode); a volatile field the design byte-pins instead of deriving (§10.6); a handler that
> would `unwrap/panic!` on a port/store failure instead of modelling the error (§10.4). Report
> each refutation with the exact capture/spec line. This is the deepest pass in the stage — use
> maximum scrutiny.

`dispatch: model=opus, effort=max` (retry once, then escalate the role to Fable per §10.10)

### 3d · Authority-reconciliation check (Opus)

> **Only for door-shadowed tokens** (F/FR/N/SCAN/NS/NSU/CS/SENT). Diff the AquaScan capture
> (wire bytes) against the `express.e` dispatch/control-flow (silent behaviour). For **any**
> divergence, do NOT auto-resolve: record it as an explicit **A/B decision** (express.e-wins
> default) tagged per facet — AquaScan owns wire bytes, `express.e` owns control-flow — in the
> design doc and `COMMAND_PARITY.md`, and **HALT to the gate** so the operator chooses (§10.3,
> §10.10). Never let a subagent pick the winner.

`dispatch: model=opus, effort=high`

**Stage-3 output artefact:** `designs/<date>-<cmd>-design.md` — what changes, how it conforms to
`specs/*.allium`, how it drives the Stage-2 captured behaviour, the grammar table, any A/B
authority decisions surfaced, and an implementation plan. §10 honored: **§10.3**, **§10.4**,
**§10.10**. Gate: present design + plan + any A/B decision; wait for approval.

---

## Stage 4 — Build (Fable implementer + Opus reviewer) · sequential build + verification workflow

### 4a · Fable implementer (test-first)

> Inputs: the approved `designs/<date>-<cmd>-design.md` (with its grammar table + plan), the
> Stage-2 captures, and `AGENTS.md`. Implement the slice test-first: a failing test pinned to the
> Stage-2 capture literals → minimum code → `cargo nextest run` → `make mutants-diff` → refactor.
> Update `COMMAND_PARITY.md` (including PLAUSIBLE rows for uncaptured/extrapolated edges) and
> `SYSTEM.md`.
>
> Discipline (all mandatory):
> - **Mutation-gate integrity (§10.1):** BEFORE `make mutants-diff`, `git add -N` every new
>   untracked file; run crate-relative (`git diff HEAD --relative` from `rust/`). **An
>   implausibly-low mutant count for the diff size FAILS the stage** — a green gate on 3-of-33 is
>   the silent trap, not a pass.
> - **Provenance (§10.4):** every wire literal carries `express.e:N` or a labelled deliberate-
>   departure note + `COMMAND_PARITY.md` row.
> - **Independent pins (§10.4):** expected literals are independent bytes — never derived from the
>   same const or `\`-continuation idiom they assert against (the D9 vacuous pin).
> - **Volatile vs stable (§10.6):** byte-pin only stable-const fields; assert format/derivation
>   for volatile fields (e.g. `== user.last_call()`-derived default, `mm-dd-yy` shape) — never the
>   captured date/time/count literal.
> - **Encoding (§10.7):** test literals use the `&str` UTF-8 code-point form; one PARITY encoding-
>   departure row per high-bit surface. Honor the wire-UTF-8 e2e gate.
> - **Failure paths (§10.4):** any handler touching a port/store gets a failing-adapter test
>   proving a *modelled error* and no partial commit on abort/EOF/idle — no `unwrap/expect/panic!`
>   on port results.
> - **Test placement (§10.4):** large/test-dominated modules → sibling `tests.rs`; small → inline;
>   never `foo_test.rs` / `#[path]`.
> - **Doc re-audit (§10.4):** re-audit existing claims in every doc section you touch — don't just
>   append; guard tests assert over full unfiltered content.
>
> If you hit an unforeseen blocker, **flag it back to the user** — do not guess.

`dispatch: model=fable, effort=high`

### 4b · Opus post-build reviewer (verification workflow)

> Inputs: the working diff, the Stage-2 captures, the Allium spec, and the touched docs. Run four
> checks and report findings:
> 1. **Mutation-gap:** any surviving mutant → run an adversarial "find the untested behaviour"
>    pass and add/strengthen tests; sanity-check the mutant count is plausible for the diff (§10.1).
> 2. **Capture-parity:** every pinned literal matches its Stage-2 capture (stable) or asserts the
>    right derivation (volatile, §10.6); every wire string has `express.e:N` provenance (§10.4).
> 3. **Allium-drift:** the implementation satisfies the Stage-1c obligations and introduced no new
>    drift.
> 4. **Doc-staleness:** `COMMAND_PARITY.md` / `SYSTEM.md` re-audited, not just appended (§10.4).
>
> Reject unprovenanced wire strings and premature port/adapter enum arms. Gate on: `nextest`
> green, warning-free `cargo build`, doctests, `mutants-diff` clean with a plausible count.

`dispatch: model=opus, effort=high`

**Stage-4 output artefact:** test-first code, updated `COMMAND_PARITY.md` + `SYSTEM.md`, clean
gates. §10 honored: **§10.1**, **§10.4**, **§10.6**, **§10.7**.

---

## Stage 5 — Compare (all Opus) · full workflow · GATE

Double-blind NextExpress-vs-FS-UAE comparison. Board serialized; NextExpress server booted with a
health check. Detailed protocol + log templates in `stage5-comparison.md`.

### 5a · Scenario author

> Inputs: the Stage-3 grammar table and Stage-2 evidence. Emit a **set** of target-agnostic
> user-behaviour scenarios — one per grammar-table row (§10.4), happy path plus the quarantined
> edge rows. Each scenario is described purely as user intent/inputs (login → join area → list
> files → …), naming no target. These feed one `pipeline` run per scenario.

`dispatch: model=opus, effort=high`

### 5b · Tester-A (NextExpress side)

> Run scenario `<N>` against the **NextExpress server over telnet** on the allocated port. Drive a
> **character-at-a-time** interactive client and record **bytes-after-each-keystroke** — echo vs
> no-echo, CR vs CRLF — not line-granular I/O (§10.7). Write a session log: (a) scenario/inputs,
> (b) target = NextExpress, (c) per-step input → what you saw. Do not read the reference log.

`dispatch: model=opus, effort=high` (parallel across scenarios — the NextExpress side is cheap)

### 5c · Tester-B (FS-UAE reference side)

> Run scenario `<N>` against the **live FS-UAE board over telnet**. Write the same session-log
> shape as Tester-A: (a) scenario/inputs, (b) target = FS-UAE, (c) per-step input → what you saw.
> Do not read the NextExpress log.
>
> **Board-serialization constraint (§10.8) — critical:** the FS-UAE board is a **singleton** with
> hazards (same-user two-node block, phantom login, node-spin on unclean close). Reference-side
> runs **MUST NOT fan out** — every Tester-B scenario is serialized through **one controlled board
> session**, with a clean `G Y` logoff between scenarios, and stays within the connection budget
> (< 5 opens before recycling the container). Never spin up a colliding board login.

`dispatch: model=opus, effort=high` (**serialized** — one at a time on the board)

### 5d · Cross-markers (double-blind)

> Inputs: for scenario `<N>`, you receive **only the other tester's** session log. Mark it against
> your own understanding of the scenario, flagging every inconsistency (missing prompt, different
> echo, spacing, line-terminator, ordering). Each log is marked by an agent holding the *other's*
> log — neither sees both raw at authoring time. Synthesize divergences into a comparison report
> under `comparison/evidence-<slice>/`. Use maximum scrutiny — this is the parity guarantee.

`dispatch: model=opus, effort=max`

### 5e · Completeness critic

> Inputs: both testers' logs + the grammar table. Ask: **what did both testers fail to exercise?**
> Name any grammar-table row, sub-prompt, or empty-collection path neither run touched (the
> "nothing missed" goal). Output the gap list; unexercised rows go back through 5a–5d within the
> loop cap (§10.8).

`dispatch: model=opus, effort=high`

### 5x · Interactive-slice human glance (§10.7) — orchestrator action, not a subagent

> If Stage 2 flagged interactive / pager / hotkey behaviour, **pause and ask the operator**
> whether to do a hands-on terminal glance before pinning — the residual per-keystroke echo the
> double-blind agents cannot observe (the D2 dead-pager class only a human caught). This is the
> agreed exception to the fully-agent-driven tradeoff.

**Stage-5 output artefact:** a double-blind comparison report under
`comparison/evidence-<slice>/`. §10 honored: **§10.7** (char-at-a-time + human glance), **§10.8**
(board serialization + connection budget + bounded loop), **§10.4** (one scenario per grammar
row). Gate: present the comparison result.

---

## Stage 6 — Resolve (Opus) · GATE

Parallel root-cause triage, one agent per divergence.

### 6a · Root-cause triage (one per divergence)

> Inputs: one divergence from the Stage-5 report, plus both session logs and the relevant
> captures/spec. In a **systematic-debugging** posture, classify it: NextExpress bug? capture
> artefact? spec ambiguity? intended departure? Propose a fix.
>
> **Confirm suspected reference ambiguity — never assume it (§10.2, §10.8):** if you suspect the
> reference is noisy/ambiguous, **re-run the reference scenario within the connection budget** to
> tell stable from noisy. If it is noisy or structurally uncapturable, resolve that facet from
> `express.e` as tiebreak, tag it *extrapolated*, and surface it. **Never auto-blame NextExpress
> code** to close a divergence. Present classification + proposed fix as a structured option.

`dispatch: model=opus, effort=high` (parallel — one dispatch per divergence)

**Stage-6 output artefact:** per-divergence classification + proposed resolution, presented to the
user as structured options. A resolution needing code loops back to **Stage 3** with an updated
plan; a clean comparison proceeds to teardown + merge (§10.9). §10 honored: **§10.2**, **§10.8**,
**§10.10**. Gate: present options; ask the user how to resolve.

---

## Dispatch quick-reference

| Brief | Role | Model | Effort |
|---|---|---|---|
| 1a | Roadmap+git reader | Opus | medium |
| 1b | Refactor-scan reader | Opus | medium |
| 1c | Allium-drift reader | Opus | medium |
| 1d | Prereq critic | Opus | high |
| 2a | Capture driver | Opus | high |
| 2b | Completeness critic | Opus | high |
| 3a | Fable designers ×2–3 | Fable | high |
| 3b | Judge panel | Opus | high |
| 3c | Adversarial refuter (Opus + one Fable) | Opus/Fable | **max** |
| 3d | Authority reconciliation | Opus | high |
| 4a | Implementer | Fable | high |
| 4b | Post-build reviewer | Opus | high |
| 5a | Scenario author | Opus | high |
| 5b | Tester-A (NextExpress) | Opus | high |
| 5c | Tester-B (FS-UAE, **serialized**) | Opus | high |
| 5d | Cross-markers | Opus | **max** |
| 5e | Completeness critic | Opus | high |
| 6a | Root-cause triage | Opus | high |
