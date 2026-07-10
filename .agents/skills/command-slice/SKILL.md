---
name: command-slice
description: Use when implementing, porting, or shipping a new NextExpress command or menu-command slice — any work that adds or changes a user-typeable AmiExpress command, its wire bytes, its Allium-spec behaviour, or its parity against the live FS-UAE / AquaScan reference (Tier A–D command slices, commands like F/FR/N/R, capture-and-compare parity work, new-files scan, file listing).
---

# command-slice

## Overview

Shipping a NextExpress command slice is a fixed ritual: **assess → capture the real
AmiExpress wire behaviour → design against the Allium specs → build test-first with
mutation gating → prove parity against the live reference.** This skill runs that ritual
as a **model-pinned, subagent-driven pipeline** with human gates at the decisions that
are yours to make.

**Core principle: capture the live board's truth before you design; never guess
user-visible behaviour from `express.e` source or a partial transcript alone.** The
recurring cost in this project has been shipping a plausible guess the live board later
refuted (N multi-conference, "AE is silent" claims, the bare-`FR` reversal, three D2 UX
defects). Every rule in `resources/hardening.md` was paid for once already.

## When to use

- Adding or changing any **user-typeable command / menu command** (dispatch table
  `amiexpress/express.e:28285`) — a new command, a re-binding, or a change to a command's
  wire bytes, grammar, or prompts.
- Any slice whose output is bytes a legacy user would see, or whose parity is pinned to
  an FS-UAE capture.

**Not for:** general debugging (use systematic-debugging), non-NextExpress work, or a
one-off fix with no wire surface.

**Refactor track:** a pure-infra / port / refactor slice with **no wire surface** (and
any pre-refactor Stage 1 names) skips Stages 2 & 5 and gates on `nextest` +
warning-free build + `make mutants-diff` + Allium-drift review. Stage 1 declares the
track. Do not force a refactor through capture-and-compare.

## The pipeline

| # | Stage | Configured role tier | Gate? | What it produces |
|---|---|---|:---:|---|
| 1 | Assess & plan | assessment | ✅ | next command + pre-refactors + track, written into `slices/` + `SLICES.md` |
| 2 | Capture truth | assessment | — | `comparison/harness/<cmd>.py`, `transcripts/<cmd>.txt`, evidence note |
| 3 | Design | generative design, independent assessment | ✅ | `designs/<date>-<cmd>-design.md` (judge-panel + adversarial refutation) |
| 4 | Build | generative implementation, independent review | — | test-first code, `COMMAND_PARITY.md`, `SYSTEM.md` |
| 5 | Compare | independent assessment | ✅ | double-blind NextExpress-vs-FS-UAE comparison report |
| 6 | Resolve | assessment | ✅ | divergence triage → your decision → loop to 3, or merge |

The configured generative roles do design and implementation; the configured assessment
roles do planning, capture, judging, comparison, and triage. Stages 3 and 5 run an
orchestration inside the stage (judge-panel / multi-scenario cross-mark) that returns one artefact, *then*
the gate fires. Per-stage detail: `resources/subagent-briefs.md`; workflow shapes:
`resources/stage3-design.md`, `resources/stage5-comparison.md`.

On a Stage 5 divergence, Stage 6 loops back to Stage 3 with an updated plan; a clean
comparison proceeds to teardown + merge.

### Dispatching subagents

Every stage role is a **versioned, client-specific agent definition** with its model and
reasoning effort pinned by that client:

- Claude Code: `.claude/agents/cs-*.md`
- Codex: `.codex/agents/cs-*.toml`

Dispatch the named `cs-*` role using the active client's native subagent mechanism. Do not
override its configured model or effort at call time. `resources/subagent-briefs.md` is the
stage-to-role index.

- **Stages 1, 2, 4, and 6** dispatch their listed `cs-*` roles directly.
- **Stages 3 and 5** orchestrate the listed roles for fan-out, bounded retry loops, and
  cross-marking; preserve the serialization constraints in the stage references.
- **Never** substitute an unconfigured generic subagent for a named role. The independent
  generator/assessor split is intentional.

## Non-negotiable invariants

These are discipline rules, not suggestions — each has been violated before and cost a
reversal or a silently-shipped bug. Full detail + evidence in `resources/hardening.md`
(§10.1–§10.10). The ones agents most often rationalize past:

| Rationalization (STOP if you think this) | Reality |
|---|---|
| "`make mutants-diff` came back clean, we're covered." | On new files it silently reports 3-of-33. `git add -N` new files, run crate-relative, and **fail on an implausibly-low mutant count** (§10.1). |
| "The capture covers it; the edges are probably the same." | The edges are exactly what got refuted. Satisfy the edge-probe battery; when an edge is uncapturable, resolve from `express.e` tagged *extrapolated* — never guess (§10.2). |
| "The AquaScan capture is the reference, I'll match it." | For door-shadowed tokens (F/FR/N/SCAN/NSU/CS/SENT) door-vs-source is **your call** — halt and ask, express.e-wins default (§10.3). |
| "I'll pin the value I saw in the transcript." | Volatile fields (dates, times, node/conf numbers) drift by capture day — assert derivation, byte-pin only stable-const fields (§10.6). |
| "The high-bit bytes match the capture." | Raw Latin-1 bytes = mojibake on a real terminal. Re-encode ≥0x80 to `&str` code points; one PARITY departure row per high-bit surface (§10.7). |
| "Two agents cross-checked the pager, echo's fine." | Agents reading telnet tokens can't see per-keystroke echo. For interactive/pager/hotkey slices, **prompt the operator for a hands-on glance** (§10.7). |
| "It compiles and tests pass, ship it." | Port/store failures need a modelled-error test, not `unwrap/panic!`; every wire literal needs `express.e:N` provenance (§10.4). |

**Red flags — stop and re-read `resources/hardening.md`:** a green mutation gate you
didn't sanity-check · byte-pinning a date/time/count · matching a door capture without
checking `express.e` · a new port/adapter enum arm · "AE is silent" without a live probe
· a guard test that pre-filters its subject before diffing · abandoning a board session
without `G Y`.

## Setup, board, and teardown

Before Stage 2, and torn down after Stage 6 — see `resources/board-lifecycle.md`:

0. **Resume check (first action):** on invocation, look for `.command-slice/run-state.json`.
   If it exists, this is a **resume** — load stage/scenario/ports/container/PID and
   **reconcile live resources first** (drain + `G Y` any open board, kill the recorded
   server PID, check `allocate_ports.py` `stale_servers`) per `board-lifecycle.md` (f), then
   continue from the recorded stage. If absent, start fresh.
1. **Worktree:** resume an in-progress slice if one exists, else create a fresh worktree
   off `origin/main`. Merge to `main` at the end (no PR) — rebase first, never move a
   `main` checked out elsewhere.
2. **Ports:** `python .agents/skills/command-slice/resources/allocate_ports.py --worktree "$PWD"`
   picks a free FS-UAE host port + a free NextExpress port and checks nothing we'd corrupt
   is already running.
3. **Board + server:** boot our own FS-UAE container (per-run name + port; never touch
   another session's board) early, during Stage 1, to hide the 2–3 min boot. Boot the
   NextExpress server for Stage 5 with a health check. **Tear both down cleanly at the
   end** — clean `G Y` logoff (node-spin hazard), kill the server by PID.
4. **Run-state:** persist stage/scenario/ports/PIDs so a stalled run resumes without
   double-booting a board or stranding a spinning node.

## Resources (load on demand)

| File | Load when |
|---|---|
| `hardening.md` | always skim before building; the §10 invariants + evidence |
| `subagent-briefs.md` | dispatching any stage's subagent (stage → `cs-*` agent dispatch index) |
| `board-lifecycle.md` | booting/driving/tearing down FS-UAE + the NextExpress server |
| `stage3-design.md` | running the Stage 3 design workflow |
| `stage5-comparison.md` | running the Stage 5 comparison workflow |
| `edge-probe-battery.md` | Stage 2 completeness check |
| `artifact-conventions.md` | writing any stage's output (where + what shape) |
| `allocate_ports.py` | setup |

Full design rationale: `docs/superpowers/specs/2026-07-04-command-slice-skill-design.md`.
