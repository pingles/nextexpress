# edge-probe-battery.md

The canonical edge-input checklist Stage 2 must satisfy before its completeness
critic can pass (§10.2). **A capture row per applicable item** — happy-path only
does not pass.

**Why this file exists:** nearly every project refutation was an *edge* the
happy-path capture missed — N was assumed single-conference/CF-gated, "AE is
silent" was assumed, bare-`FR` shipped then reversed (`fa2a855`, 9 files), the D9
date-prompt edges were guessed from source. The edges are exactly what gets
refuted. Capture them, or resolve from source and tag *extrapolated* — never guess
from partial bytes.

## Standard fallback clause

Every applicable row ends the same way. Restate it in the evidence note per row:

> **If uncapturable** (timeout / door-pager consumption / two-node block) resolve
> the facet from `express.e` control-flow and tag *extrapolated-from-source*
> (§10.2) in `COMMAND_PARITY.md`. **Do NOT guess.** Cite the line
> (`displayFileList:27626`, `getDirSpan:26857`, the handler's dispatch at
> `express.e:28285`).

Uncapturable ≠ skippable: it still gets a row, tagged, with a source citation.

## The battery — one row per applicable class

Run each class **at every prompt the command exposes**: the top-level command
letter, then each sub-prompt it opens. Record command → keystrokes → bytes seen.

| # | Edge class | What to type | What to observe |
|---|---|---|---|
| 1 | Empty input | bare Enter at the prompt | Enter-default vs re-prompt vs abort/exit. Is empty a *cancel* or a *default value*? |
| 2 | Whitespace-only | one or more spaces then Enter | Trimmed to empty (→ row 1) or treated as a token? Distinct from bare Enter? |
| 3 | Bare-CR vs bare-LF | send `\r` alone, then `\n` alone (separate probes) | Does each terminate the line? Same behaviour? One ignored? Matters for line-read prompts and hotkey reads alike. |
| 4 | Out-of-range number | a number past the list/range end (e.g. `999`) | Error text, clamp, re-prompt, or silent ignore. Capture the exact message. |
| 5 | Non-numeric where number expected | letters at a numeric prompt (e.g. `abc`) | Reject text vs re-prompt vs treated-as-zero. Distinct from row 4? |
| 6 | Unknown command / token | a letter not in the dispatch table; a garbage sub-verb | Menu re-display, error, or silent swallow. Compare to the door-shadow caveat (below). |
| 7 | Trailing junk after a valid arg | `R <date>`, `R 1 garbage`, extra tokens | Is the tail **discarded**, parsed, or an error? (Legacy `R` discards the date arg — capture it, don't assume.) |
| 8 | Lone-key vs full-line read | type one key **without** Enter; separately type key+Enter | Does the prompt act on the single keypress (hotkey read) or wait for Enter (line read)? Governs Stage-5 character-at-a-time driving (§10.7). |
| 9 | Sub-prompt accept set vs reject set | at each sub-prompt, every documented verb (accept) **and** an off-list key (reject) | The full accept alphabet and what a reject does. E.g. `flagFiles`: name→flag, `C`→clear+reloop, Enter→none; the yes/no confirm's single-key `Y`/`N`/other. |
| 10 | Empty-collection gate | trigger the command against an **empty dir / empty list / nothing flagged** | The gate path text (e.g. "nothing flagged", empty-scan message) — a distinct branch from the populated path. `** AutoSaving File Flags **` proved *unconditional* even with nothing flagged. |
| 11 | Enter-default (stable) | bare Enter where a default is offered | The default value **and** whether Enter accepts it. Byte-pin only if stable-const (§10.6). |
| 12 | Enter-default (VOLATILE) | bare Enter at a last-call / date / node default | The default is **runtime-derived** (e.g. last-call date `mm-dd-yy`). Capture the *derivation*, never the literal — `06-25-26` drifts by capture day (§10.6). Tag volatile-runtime. |
| 13 | Case sensitivity | the command / sub-verb in lower vs upper case | Is `n` == `N`? `y` == `Y`? Per prompt — do not assume uniform. |
| 14 | Singular / plural + inline-arg forms | the bare command, then every inline form: `N !1`, `Z ART 1`, `FR`, numbered/ranged args | Each accepted form and its handling. The Stage-3 grammar table needs **one row per form** (§10.4); Stage 5 runs one scenario per row. A dropped inline-arg is exactly the D4→D7 same-day patch. |

## Per-prompt discipline

- **Enumerate prompts first.** Read the handler at `express.e:28285` dispatch →
  its sub-prompt calls. Every prompt (top-level + each sub) gets the full battery.
- **Accept set AND reject set** (row 9) are both mandatory: knowing what a prompt
  *rejects* is as load-bearing as what it accepts, and the reject branch is a
  common refutation site.
- **Tag every field** stable-const vs volatile-runtime as you capture (§10.6), and
  **flag** any interactive / pager / hotkey behaviour for the Stage-5 human-glance
  prompt (§10.7).

## Door-shadow caveat (affects rows 6, 9, 14)

For door-shadowed tokens (`F`/`FR`/`N`/`SCAN`/`NS`/`NSU`/`CS`/`SENT`) the stock
AquaScan/NextScan door — not `internalCommand*` — answers the token, so your
capture is of the **door**, not the genuine internal command. Capture both facets:
AquaScan owns the **wire bytes**, `express.e` owns the **silent control-flow**. Any
divergence is a §10.3 door/source decision — **halt and surface it at the Stage-3
gate**, express.e-wins default. Do not auto-resolve. Every other token (`Z`
included) captures the genuine internal command.

## Grounded instances (this project has been refuted on each)

- **N** — assumed single-conference and CF-gated; live board showed neither.
  Inline form `N !1`, last-call default (VOLATILE, row 12), two page-1 models.
- **bare-`FR`** — shipped matching the door capture, reversed two days later once
  `express.e` control-flow was diffed (§10.3).
- **D9 date-prompt** — edges (empty, junk, out-of-range) guessed from source;
  resolve uncapturable date-prompt facets from `getDirSpan:26857` /
  `displayFileList:27626`, tagged *extrapolated*.
- **D2** — pager echo / double-Enter, Latin-1 mojibake, flag silence: the
  interactive-echo class (row 8, §10.7) only a human glance caught.
- **"AE is silent" claims** — repeatedly asserted, repeatedly false; row 10 and a
  live probe are mandatory before any silence claim.

## Pass condition

The Stage-2 completeness critic passes only when **every applicable class above has
either a capture row or a source-extrapolated tagged row**, per prompt. Bounded by
§10.8: cap the re-probe loop — a structurally-uncapturable item resolves from
source and moves on; it does not loop forever.
