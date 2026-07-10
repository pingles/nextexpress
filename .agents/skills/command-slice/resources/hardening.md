# hardening.md — the §10 discipline checklist

Operator-checklist form of the design spec's §10 (Hardening requirements). Every item
is a correction this project has **already paid for**. Skim this before every build; run
down it whenever a stage is about to gate. Rules are numbered exactly as the spec:
§10.1–§10.10. Cross-refs: `edge-probe-battery.md` (§10.2), `board-lifecycle.md` (§10.5/§10.8),
`stage5-comparison.md` (§10.7), `artifact-conventions.md` (§10.6/§10.7), `subagent-briefs.md`.

## Run-down checklist

| # | Sev | Rule (imperative) | Mechanism to satisfy it | Cost paid (historical) |
|---|---|---|---|---|
| §10.1 | HIGH | Never trust a green `make mutants-diff` on new files. | `git add -N` every new/untracked file first; run crate-relative (`git diff HEAD --relative` from `rust/`); **fail the stage if the mutant count is implausibly low for the diff** (rule of thumb: ~1 mutant per few changed non-trivial lines; a single-digit count over a multi-function / new-file diff is a red flag — cross-check `cargo mutants --list` for the changed files). | Fresh `commands.rs` reported 3 mutants, not 33 — green gate over untested code (untracked miss, 2026-07-03; `[cargo-mutants-in-diff-paths]`). Trips **every** run. |
| §10.2 | HIGH | Capture the edges; fall back to `express.e` only when an edge is uncapturable. | Stage 2 completeness critic mechanically satisfies the **edge-probe battery** (`edge-probe-battery.md`) — one capture row per applicable item. If structurally uncapturable (timeout, door-pager eats the command, two-node block), resolve from control-flow (`express.e` `displayFileList:27626`, `getDirSpan:26857`), tag *extrapolated-from-source* in `COMMAND_PARITY.md`. | Edge guessed from happy-path capture/source read, then refuted: N multi-conf / CF-gating, "AE is silent" claims, `** AutoSaving **` unconditional, D9 date-prompt edges. Most frequent failure class. |
| §10.3 | HIGH | Door-vs-source conflict is a **human** decision — surface it, never auto-resolve. | Stage 3 diffs AquaScan capture against `express.e` dispatch/control-flow for door-shadowed tokens (F/FR/N/SCAN/NSU/CS/SENT). **Any divergence HALTS → A/B gate**, express.e-wins default. Tag authority **per facet** (AquaScan owns wire bytes, `express.e` owns silent control-flow). Record captured-vs-extrapolated in design doc + `COMMAND_PARITY.md`. | Bare `FR` shipped to the capture, reversed two days later — commit `fa2a855`, 9 files (`[use-original-amiexpress-code]`, `[tierd-aquascan-parity-target]`). |
| §10.4 | MED | Enforce provenance + grammar + no-drift on every wire touch. | See the six sub-checks below. | Advertise-then-reject drift; D7 dropped inline-arg; ~7 "don't panic on save fail" commits; deleted `Silent` EchoMode; D9 vacuous pin; stale "flip private default to Y" note. |
| §10.5 | MED | The server has a lifecycle **symmetric to the board**; runs are resumable. | Boot NextExpress on the allocated port with a health check; register for teardown; **kill by recorded PID on run-end and on any stage failure**. `allocate_ports.py` detects a stale server squatting a port. Update a **run-state file** (stage, scenario index, ports, container name, server PID) at each stage boundary; on resume **reconcile live resources first** (drain + `G Y` any open board, kill stale server) before continuing. Non-user-facing/refactor track skips Stages 2 & 5. | Orphaned server + double-booked board (two-node block / DoS-ban) on a naive resume. |
| §10.6 | MED | Byte-pin only stable-const fields; assert *derivation* for volatile ones. | Stage 2 tags each captured field stable-const (glyphs, prompts, dash geometry) vs volatile-runtime (dates, times, node/conf numbers, last-call-derived defaults). Stage 4 byte-pins stable-const; volatile fields assert format/derivation (`== user.last_call()`-derived default, `mm-dd-yy` shape) — never the captured literal. | The `06-25-26` date default drifts by capture day → a byte-pin would break on the next run. |
| §10.7 | MED | Re-encode high-bit bytes; keep the interactive-echo human residual. | Record every byte ≥0x80 with **both** its Latin-1 byte and target UTF-8 code point; Stage-4 literals use the `&str` code-point form; one `COMMAND_PARITY.md` encoding-departure row per high-bit surface. Tester-A drives NextExpress **character-at-a-time** (bytes-after-each-keystroke). If Stage 2 flags interactive/pager/hotkey behaviour, **prompt the operator for an optional hands-on glance** before pinning. | Naive verbatim paste = mojibake before the e2e gate exists; the D2 dead-pager class only a human caught. |
| §10.8 | MED | Cap every loop and the connection count; resume stalls, don't re-prompt. | Cap Stage-2 re-probes, Stage-3 judge/refutation rounds, Stage-5 per-scenario pairs — a loop at its cap **escalates to a human gate**, never spins. Enforce the **telnet connection budget: < 5 opens before recycling the container** and **per-stage token/effort budgets** to bound cost. Subagent stall (completed-tool-then-silence = API outage) → **resume, not re-prompt**; `G Y` any open board first; no `timeout(1)` on this host — use the Monitor/until-loop pattern. | DoS self-ban stranding a Stage-5 run mid-comparison (`[workflow-cargo-stall-hazard]`). |
| §10.9 | MED | Merge to `main`, no PR — but rebase and reconcile shared docs. | Rebase on `origin/main` first; **never move a `main` checked out in another worktree** (merge from a quiescent vantage); reconcile the high-contention docs (`COMMAND_PARITY` / `SYSTEM` / `SLICES` / `AGENTS`) at stable anchors and **re-audit after rebase**. | Same-region appends to the shared docs still conflict — the "additive edits never collide" assumption is false for exactly those files. |
| §10.10 | MED | Gates present **decisions**, not artefacts to rubber-stamp; model failures escalate. | Any door/source (§10.3), capture/source, or volatile-field ambiguity → explicit choice at the gate with express.e-wins default, recorded captured-vs-extrapolated. Model failure (a generative role cannot produce a compilable result; the independent assessment role cannot reconcile it) → **retry once → dispatch the configured assessment role → halt to a human gate**. | The least-bad candidate shipping silently; a loop hiding a model failure as "converged". |

### §10.4 sub-checks (run all of these in the Stage-4 brief + post-build review)

- [ ] **Binding provenance** — every letter the slice binds cites its `express.e:28285` dispatch line or is a labelled departure; menu-asset rows diffed **verbatim**, no token pre-filter.
- [ ] **String provenance** — every user-facing literal carries `express.e:N` or a labelled deliberate-departure note + `COMMAND_PARITY.md` row; review rejects unprovenanced wire strings.
- [ ] **Grammar table** — Stage-3 design enumerates every input form (bare / inline-arg / each sub-prompt + verb set); adversarial pass hunts an accepted form with no row; Stage 5 runs **one scenario per row**.
- [ ] **Failure-path** — any handler touching a port/store has a failing-adapter test proving a *modelled error* and no partial commit on abort/EOF/idle; review grep-rejects `unwrap/expect/panic!` on port results.
- [ ] **No self-referential pins** — expected literals are independent bytes, never derived from the same const / `\`-continuation idiom they assert against.
- [ ] **Doc-audit** — for every doc section the slice touches, **re-audit existing claims** against code / `express.e`, not just append; guard tests assert over full unfiltered content.
- [ ] **No premature abstraction** — a new port/adapter enum arm must cite a Stage-2 behaviour unreachable with existing seams; adversarial pass tries to show it is reachable without.
- [ ] **Test placement** — large/test-dominated module → sibling `tests.rs`; small → inline; never `foo_test.rs` / `#[path]` (`[test-placement-convention]`; memory-only, so the brief must restate it).

## Red flags — STOP

Stop and re-read the relevant § above the moment you notice any of these:

- A green mutation gate you **didn't** sanity-check the mutant count of → §10.1
- Running `make mutants-diff` with new files **not** `git add -N`'d, or from repo-root ("No mutants to filter") → §10.1
- Byte-pinning a date, time, node number, conf number, or last-call-derived default → §10.6
- Matching a **door** capture (F/FR/N/SCAN/NSU/CS/SENT) without diffing `express.e` → §10.3
- "AE is silent / AE rejects / AE accepts" asserted **without** a live probe → §10.2
- An edge behaviour taken from a happy-path transcript or a bare source read → §10.2
- A guard/pin test that **pre-filters** its subject before diffing (token filter, string slice) → §10.4
- Pinning an expected literal **derived from the same const** it asserts against → §10.4
- A new port/adapter enum arm with no cited Stage-2 behaviour that needs it → §10.4
- `unwrap` / `expect` / `panic!` on a port or store result → §10.4
- Raw bytes ≥0x80 pasted verbatim from a Latin-1 capture into a Rust literal → §10.7
- An interactive / pager / hotkey slice pinned with **no** operator terminal glance → §10.7
- Abandoning a board session without a clean `G Y` logoff → §10.8, `board-lifecycle.md`
- A convergence loop past its cap that keeps spinning instead of escalating → §10.8
- A resume that boots a second board before draining the first → §10.5
- Appending to `COMMAND_PARITY` / `SYSTEM` / `SLICES` / `AGENTS` without re-auditing after rebase → §10.9

## Rationalization table (discipline-critical)

Each row: the tempting shortcut, the **specific wrong move** it licenses, and the reality
that closes the loophole. If your reasoning matches the left column, you are about to pay
a cost this project already paid.

| You're thinking… | The wrong move it licenses | Reality (do this instead) |
|---|---|---|
| "`make mutants-diff` came back clean, we're covered." (§10.1) | Reading the exit status, not the count; skipping `git add -N`; running from repo-root. | On new files the gate silently reports **3-of-33** — a green light over untested code. `git add -N` every new file, run crate-relative from `rust/`, and treat an **implausibly-low count as a failure, not a pass**. A low count means the tool didn't see your code, not that your code is bulletproof. |
| "The capture covers it; the edges are probably the same." (§10.2) | Pinning empty-input / out-of-range / unknown-token / trailing-junk behaviour from the happy-path transcript, or from a quick `express.e` read. | The edges are **exactly** what got refuted (N multi-conf, CF-gating, "AE is silent", D9 date edges). Satisfy the full edge-probe battery with a capture row each. "Uncapturable" is **not** a licence to guess: it's a licence to resolve the *facet* from `express.e` control-flow, tagged *extrapolated* — and only after you've proven it's structurally uncapturable, not just inconvenient. |
| "The AquaScan capture is the reference, I'll just match it." (§10.3) | A subagent silently pinning a door-shadowed token to the door's wire bytes, treating the conflict as resolved. | For F/FR/N/SCAN/NSU/CS/SENT the door and `express.e` genuinely conflict and **which wins is the operator's call**, not yours and not a subagent's. Any divergence **HALTS** and goes to the gate as an A/B decision (express.e-wins default), tagged per facet: door owns wire bytes, `express.e` owns silent control-flow. Bare `FR` shipped to the capture and was reversed two days later (`fa2a855`) — that is the price of skipping the gate. |
| "I'll pin the value I saw in the transcript." (§10.6) | Byte-pinning a date, time, node/conf number, or last-call-derived default because it's right there in the capture. | Volatile fields **drift by capture day** — the `06-25-26` default is different tomorrow. Byte-pin only stable-const fields (glyphs, prompts, dash geometry). For volatile fields assert the **derivation/format** (`== user.last_call()`-derived, `mm-dd-yy` shape). A byte-pin on a volatile field is a test that passes today and lies next week. |
| "The high-bit bytes match the capture, paste them in." (§10.7) | Copying raw Latin-1 bytes ≥0x80 straight into a Rust literal because they're byte-equal to the transcript. | Raw Latin-1 bytes render as **mojibake** on a real UTF-8 terminal and blow the e2e UTF-8 gate. Re-encode each ≥0x80 byte to its **UTF-8 code point** in a `&str` literal (`\xa9` → `\u{a9}`), record **both** forms, and add one `COMMAND_PARITY.md` encoding-departure row per high-bit surface. Byte-equality with a Latin-1 capture is the bug, not the goal — wire parity is at the code-point boundary. |

## The one-line version

Capture the live board's truth before you design; never guess user-visible behaviour from
`express.e` or a partial transcript. When you must fall back to source, say so
(*extrapolated*). Every rule here was paid for once already — don't buy it twice.
