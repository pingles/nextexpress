# Stage 5 — Compare · full Workflow · GATE

Prove the built command matches the live reference in **behaviour and on-screen
experience**, double-blind. The configured assessment roles run an orchestration inside the
stage that returns one comparison report, and *then* the gate fires.

Prereqs: Stage 4 done (`nextest` green, warning-free build, `mutants-diff` clean); the
NextExpress server booted on the allocated port with a passing health check
(`board-lifecycle.md`); the FS-UAE board still up from Stage 1. The grammar table from the
Stage 3 design (§10.4) is the scenario source-of-truth.

## Roles

| Role | Count | Job |
|---|---|---|
| Scenario author | 1 | Emit a **set** of target-agnostic user-behaviour scenarios: one per §10.4 grammar-table row + happy path + each quarantined edge row. Pure user intent/inputs — no mention of NextExpress vs FS-UAE. |
| Tester-A | N (parallel) | Drive **NextExpress/telnet**, character-at-a-time (§10.7). One per scenario, fan out freely. |
| Tester-B | N (**serialized**) | Drive **live FS-UAE/telnet**. One controlled board session, clean `G Y` each time, connection budget < 5 (§10.8). |
| Cross-marker | 2N (parallel) | Double-blind: mark each log while holding the *other's* log. |
| Completeness critic | 1 | "What did **both** testers fail to exercise?" against the grammar table + edge-probe battery. |

## Board constraint (§10.8) — the one hard serialization

The FS-UAE board is a **singleton** with hazards: same-user two-node block, phantom login,
node-spin on unclean close, DoS self-ban after too many opens. Therefore:

- **Tester-B runs are serialized** through one board session — never fan out reference-side
  logins. Each Tester-B scenario ends with a clean `G Y` logoff.
- **Connection budget < 5 telnet opens** before recycling the container.
- **Tester-A (NextExpress) and every cross-marker parallelize** — the cheap side is free.
- Batch all Tester-B runs behind one board lease; do not interleave them with teardown.

## Interactive human-glance prompt (§10.7)

If the **Stage-2 note flagged interactive / pager / hotkey** behaviour, **pause and ask the
operator** whether to do an optional hands-on terminal glance before pinning. The
double-blind agents read telnet tokens and cannot observe true local per-keystroke echo —
the D2 dead-pager class only a human caught. This is the agreed exception to the otherwise
fully-agent-driven verification.

## Tester-A specifics (§10.7) — character-at-a-time

Tester-A must drive a **character-at-a-time interactive client**, not line-granular I/O:

- **Reference technique to port:** `comparison/harness/ae_tierd_probes.py` (P3 feeds a prompt
  one byte at a time to observe per-keystroke echo) is the shape to adapt to the NextExpress
  side; take the telnet IAC / SUPPRESS-GO-AHEAD (character-mode) option handling from
  `bbsdrive.py`. The line-granular `rust_*.py` drivers (`read_until_any`) do **not** satisfy
  §10.7 and must not be the Tester-A base.
- **How echo is observed over telnet:** send one byte, read what comes back — the sent byte
  returned = echo, anything else = server output. A line-read prompt echoes each key; a
  hotkey / lone-key read echoes nothing.
- Send one keystroke at a time; **log the bytes received after each keystroke**.
- Record **echo vs no-echo** per key (hotkey/lone-key surfaces echo nothing; line-reads echo).
- Record the **line terminator** actually emitted: **CR vs LF vs CRLF**.
- Note bare-CR / bare-LF handling and lone-key-vs-line-read boundaries.
- Confirm every byte decodes as valid UTF-8 (the wire is always UTF-8; ≥0x80 as `&str`
  code points, never raw Latin-1).

## Orchestration sketch

> This pseudocode describes dependencies, not a client API. Dispatch each named `cs-*` role via
> the active client's native mechanism and preserve its configured model and reasoning effort.
> `cs-tester-ref` stays serialized on the singleton board (§10.8).

```
workflow stage5_compare:
  # 1. serial: author the scenario set (`cs-scenario`)
  scenarios = author_scenarios(grammar_table, happy_path, quarantined_edges)
    # one scenario object per grammar-table row + happy + each edge

  # 2. reference side: SERIALIZED behind one board lease (§10.8)
  logs_B = []
  with board_lease(container, budget<5):
    for s in scenarios:                       # strictly sequential
      logs_B.append(TesterB(s, target="FS-UAE"))   # clean `G Y` after each

  # 3. NextExpress side: PARALLEL (cheap, no singleton)
  logs_A = parallel[ TesterA(s, target="NextExpress") for s in scenarios ]

  # 4. double-blind cross-mark: PARALLEL, each marker holds the OTHER log
  marks = parallel[
    CrossMark(log=logs_A[i], reference=logs_B[i]) for i in scenarios
  ] + parallel[
    CrossMark(log=logs_B[i], reference=logs_A[i]) for i in scenarios
  ]

  # 5. completeness critic (`cs-completeness-critic`): what did BOTH miss?
  gaps = CompletenessCritic(scenarios, logs_A, logs_B, grammar_table, edge_battery)
  # cap re-probe rounds (§10.8); on cap -> escalate to gate, don't spin

  # 6. synthesize -> comparison report -> GATE
  report = synthesize(marks, gaps)
  write("comparison/evidence-<slice>/comparison-<date>.md", report)
  present_gate(report)
```

Optional operator step between 1 and 2: fire the §10.7 human-glance prompt if Stage 2
flagged interactive/pager/hotkey.

## Template — scenario (target-agnostic, from the author)

```
### Scenario S<n>: <short name>   [grammar-row §10.4: <row> | happy | edge:<which>]
Intent: <what the user is trying to do, in plain terms>
Preconditions: <logged-in state, conference joined, files flagged, etc.>
Inputs (keystrokes, in order):
  1. <key/line>
  2. <key/line>
  ...
Expected user-visible shape (NOT byte-pinned here): <prompt appears, list scrolls, …>
```

One scenario **per grammar-table row**, plus the happy path, plus each quarantined edge
(empty/junk, out-of-range, unknown token, trailing junk, bare-CR/LF, empty-collection gate).

## Template — session log (per tester, per scenario)

```
## Session log — S<n> — target: <NextExpress | FS-UAE>
a) Scenario / inputs: S<n> <name>; inputs = [ <keystrokes> ]
b) Target: <NextExpress @127.0.0.1:PORT | FS-UAE @127.0.0.1:PORT>
c) Per-step:
   | # | input (key/line) | observed bytes after keystroke | echo? | terminator | notes |
   |---|---|---|---|---|---|
   | 1 | J        | "Conf:"           | no   | —    | hotkey, no echo |
   | 2 | 2\r      | "2\r\n"           | yes  | CRLF | line-read echoes |
   | 3 | ...      | ...               | ...  | ...  | ... |
Session end: clean `G Y` logoff [Tester-B] / connection closed [Tester-A]
```

Tester-A always fills echo/terminator columns per keystroke (§10.7). Tester-B records the
same fields from the live board.

## Template — cross-mark (marker holds the OTHER tester's log)

```
## Cross-mark — S<n> — this log: <target> — against: <other target>
Divergences flagged:
| # | step | this-log observed | other-log observed | severity | kind |
|---|---|---|---|---|---|
| 1 | 2 | "2\r\n" | "2\n" | MAJOR | terminator (CRLF vs LF) |
| 2 | 4 | "\u{a9}1994" | "\xa9..." | BLOCKER | encoding (mojibake risk) |
Clean steps: <list, or "all remaining match">
```

**Severity ladder:** `BLOCKER` (wrong bytes / crash / mojibake / missing prompt) ·
`MAJOR` (terminator/echo mismatch, wrong ordering) · `MINOR` (whitespace/spacing a user
would not notice) · `INFO` (volatile-field difference expected to drift — date/time/node,
§10.6; note, do not fail on it).

## Comparison report shape

Written to `comparison/evidence-<slice>/comparison-<date>.md`:

```
# Comparison — <slice>/<cmd> — <date>
Server: NextExpress @PORT (commit <sha>)   Reference: FS-UAE <container> (budget: k/5 opens)
Scenarios: <count> (grammar rows <n> + happy + edges <m>)
Interactive human glance: <done / declined / not applicable> (§10.7)

## Verdict
<CLEAN — no un-triaged divergence> | <N divergences -> Stage 6>

## Divergences (from cross-marks, most-severe first)
| id | scenario | step | severity | kind | NextExpress | FS-UAE | note |
...  (INFO/volatile rows called out as expected drift, §10.6)

## Completeness-critic findings
What BOTH testers failed to exercise: <grammar rows / edge-battery items uncovered>
Re-probe outcome: <covered on re-run | capped+escalated (§10.8) | uncapturable->express.e §10.2>

## Evidence
Session logs: <paths under comparison/evidence-<slice>/>
Transcripts referenced: comparison/transcripts/<cmd>.txt
```

## Gate

Present the report as a **structured decision** (§10.10):

- **CLEAN** (no un-triaged divergence, completeness critic satisfied) → proceed to teardown
  + merge (`board-lifecycle.md`, `artifact-conventions.md`).
- **Any divergence** → hand the divergence rows to **Stage 6** (`subagent-briefs.md`) for
  root-cause triage; do not auto-blame NextExpress and do not rubber-stamp.

If a convergence loop hits its cap (§10.8) — completeness re-probe or a Tester-B retry
against the connection budget — **escalate to this gate**, do not spin.
