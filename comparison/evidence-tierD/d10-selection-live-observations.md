# Live observations — Slice D10 listed-file selection index

Captured **2026-07-10** against the genuine AmiExpress 5.6.0 board in the
isolated FS-UAE container `nextexpress-ref-nextscan-index` on
`127.0.0.1:30569`. The stock `F` / `FR` / `N` command icons run **AquaScan
v1.0**, before `processInternalCommand` (`express.e:28229-28256`), so these
bytes describe the door rather than `displayFileList`.

- Driver: [`comparison/harness/ae_tierd_d10_selection.py`](../harness/ae_tierd_d10_selection.py)
- Transcript: [`comparison/transcripts/ae_tierd_d10_selection.txt`](../transcripts/ae_tierd_d10_selection.txt)
- Bounded completeness re-probe driver:
  [`comparison/harness/ae_tierd_d10_edge_reprobe.py`](../harness/ae_tierd_d10_edge_reprobe.py)
- Re-probe transcript:
  [`comparison/transcripts/ae_tierd_d10_edge_reprobe.txt`](../transcripts/ae_tierd_d10_edge_reprobe.txt)
- Fixture: conference 2 with the committed Tier-D `Dir1` and `Dir2` catalogues;
  the isolated conference metadata was changed to `NDIRS=2`.
- The authoritative run used one socket. The whole campaign used four opens,
  including discarded setup/driver diagnostics; all ended with `G Y`. The
  authoritative logoff is at transcript lines 998-1005:
  `** AutoSaving File Flags **` -> BEL -> `Click...` -> EOF.
- The completeness critic required one bounded retry. The four-open container
  was recycled first, then one session captured E1-E3. The populated-HOLD E4
  attempt timed out after `R`; its partial bytes were not emitted as a block,
  and the driver's final `G Y` was swallowed by the still-active pager. The
  post-run audit restored Dir2 (257 bytes) and the empty Hold/Held (0 bytes),
  then recycled the isolated container to clear the phantom node. **No HOLD
  result and no clean-logoff claim are made for the re-probe.** The original
  authoritative transcript remains the clean campaign record.
- Stage-1 gate: the operator chose board-as-shipped AquaScan authority for the
  pager and a bounded D10 registry refactor. Exact duplicate-file identity is
  still item 30 / D2s work.

## Headline findings

1. **Numeric selection is directory-local.** In `F A`, directory 1 emitted its
   own `File #1`, then directory 2 restarted at `File #1`; `R` + `1` while on
   directory 2 flagged `FRESHUPL.LHA`, not directory 1's `ANSIPACK.LHA`
   (lines 188, 194-195, 340, 346-358, verified at 388-398).
2. **`FR A` traverses directories 1 then 2, but reverses rows inside each
   directory.** Directory 2's `File #1` was `TOOLPACK.LHA`; `R 1` selected that
   name (lines 435-605, verified at 635-645). D10 must not reverse the directory
   span while fixing the registry.
3. **An empty directory transition replaces the numeric selection view.** `N`
   with `06-10-26` found nothing in directory 1, then directory 2 numbered
   `MYDEMO.DMS` as `File #1`; `R 1` selected `MYDEMO.DMS` (lines 675-698,
   verified at 728-738).
4. **Reload re-renders the current directory from `File #1`.** `L` after the
   end of `F 2` cleared the screen and emitted directory 2 again with the same
   `File #1..#3` mapping (lines 768-812). The catalogue was not mutated during
   the live session, so stale-row removal after a changed reload is not a live
   claim; Stage 4 must pin that reconciliation with an in-memory catalogue
   mutation test.
5. **Numeric parsing accepts a valid whitespace token and ignores junk tokens.**
   Empty, `999`, and `abc` returned to `More?` without selecting. The final
   numeric input was `1 garbage`—there was no later plain `1`—and the subsequent
   `A` listed `FRESHUPL.LHA`. Thus AquaScan resolved whitespace token `1` and
   ignored the separate trailing `garbage` token (lines 812 and 842-852).
6. **AquaScan's name prompt is not a catalogue lookup.** Empty input selected
   nothing, but lower-case `nosuch.lha` was accepted and later rendered by
   internal `A` as `NOSUCH.LHA`. `mydemo.dms` plus trailing spaces was trimmed,
   normalized and accepted. Verification was exactly
   `FRESHUPL.LHA NOSUCH.LHA MYDEMO.DMS` (lines 812 and 842-852).
7. **The fixture cannot demonstrate held-row selection.** `F H` returned
   `Scanning HOLD dir from top... Nothing found!`; the following `A` showed
   `No file flags` (lines 882-929). Allium nevertheless requires a flagged
   `File` to have status `{available, lcfiles}` (`specs/files.allium:187-203`).
   Legacy name flagging has no status lookup (`express.e:12523-12542`), so held
   selection cannot be labelled source-extrapolated rejection. It remains an
   explicit Allium-vs-legacy question; the live capture proves only the empty
   hold gate.
8. **Flag entries are CR-terminated line reads.** At both number and name
   prompts, a prefix echoed during an idle observation but did not submit until
   CR. Whitespace-only plus CR selected nothing. A bare LF produced an LF byte
   but did not submit; the still-open prompt required a recovery CR
   (`ae_tierd_d10_edge_reprobe.txt:206-209,304-307`).
9. **Numeric input is plural and duplicate-idempotent.** `1 2 1` selected
   `FRESHUPL.LHA` and `MYDEMO.DMS` once each, in first-occurrence order
   (`edge_reprobe.txt:206-244`). This combines with finding 5: valid decimal
   whitespace tokens resolve; invalid/out-of-range tokens are ignored. The
   unspaced form `1garbage` was not probed and is not inferred.
10. **Name input preserves the whole non-empty line.** The door accepted
    `freshupl.lha mydemo.dms freshupl.lha`; internal `A` rendered the normalized
    line `FRESHUPL.LHA MYDEMO.DMS FRESHUPL.LHA`
    (`edge_reprobe.txt:304-342`). Wire output alone cannot expose item
    boundaries, but legacy `flagFiles` passes the entire line once to
    `addFlagToList` (`express.e:12638`), which stores one trimmed/upper-cased
    string (`express.e:12523-12542`). Current NextExpress instead splits the
    name line into catalogue tokens.
11. **Changed-data reload replaces numeric identity.** E3 first rendered the
    committed Dir2 (`FRESHUPL.LHA` as #1), replaced its catalogue with Dir1,
    used `L`, observed `ANSIPACK.LHA` as the new #1, then `R 1`; internal `A`
    verified `ANSIPACK.LHA` (`edge_reprobe.txt:377-466`). Stale pre-reload #1
    must not win.

## Authority divergence discovered

The captured name-input behaviour in finding 6 conflicts with the Allium
`FlagFile(s, file)` model: Allium requires an existing `File` with an allowed
status and download access (`specs/files.allium:187-203`), whereas AquaScan
persisted the non-existent `NOSUCH.LHA`. The legacy source agrees with the door:
`addFlagToList` full-trims and uppercases any input longer than one character,
then appends it without a catalogue lookup (`express.e:12523-12542`), and the
internal pager routes `F` to that `flagFiles` path (`express.e:28025-28058`).
There is no internal numeric-`R` equivalent.

This is therefore an **Allium-vs-captured/legacy semantics** decision, not a
door-vs-source split. Stage 3 must surface it explicitly: either preserve the
captured and source-backed unchecked name flag, documenting an Allium departure,
or strengthen the legacy behaviour to Allium's resolved/downloadable `File`
precondition. It must not be silently folded into the registry design.

## Capture matrix

| Scenario | Input / pager actions | Result | Transcript |
|---|---|---|---|
| Initial state | internal `A`, clear `*` | `No file flags`; deterministic empty start | 141-151 |
| Forward cross-directory | `F A`; after directory 2 `File #1`, `R`, `1`, `Q` | `FRESHUPL.LHA` | 181-398 |
| Reverse cross-directory | `FR A`; after directory 2 reverse `File #1`, `R`, `1`, `Q` | `TOOLPACK.LHA` | 428-645 |
| Empty-dir transition | `N`; date `06-10-26`; dirs `A`; directory 2 `R 1` | `MYDEMO.DMS` | 675-738 |
| Reload / selection grammar | `F 2`; `L`; numeric empty / `999` / `abc` / `1 garbage`; name empty / `nosuch.lha` / `mydemo.dms   ` | numeric token `1` plus both non-empty names persisted | 768-852 |
| Empty hold area | `F H` | `Nothing found!`; no flags | 882-929 |
| Session close | `G Y` | autosave, carrier drop, EOF | 998-1005 |
| Numeric edge retry | `R`; spaces+CR; LF then recovery CR; byte `1`, idle, then ` 2 1`+CR | whitespace ignored; LF does not submit; first two distinct numbers selected | edge-reprobe 181-244 |
| Name edge retry | `F`; spaces+CR; LF then recovery CR; partial then `freshupl.lha mydemo.dms freshupl.lha`+CR | whitespace ignored; LF does not submit; whole normalized line persisted | edge-reprobe 279-342 |
| Changed reload retry | `F 2`; replace Dir2 with Dir1; `L`; `R 1` | new Dir1-derived #1 `ANSIPACK.LHA` selected; fixture restored | edge-reprobe 377-466 |
| Populated HOLD retry | seed Hold/Held; `F H`; `R` | timed out before a number prompt; no emitted evidence block; fixture restored; container recycled | **unresolved at bounded-loop cap** |

## Stable, volatile and encoded fields

| Field | Treatment |
|---|---|
| `More?`, `File number(s) to flag:`, `File name(s) to flag:` and `File #` framing | **stable-const**, already owned by the earlier file-list slice; D10 adds no new literal |
| Per-directory restart at 1; current-directory numeric resolution; reload replacement | **stable behaviour**, assert from a derived fixture mapping rather than by searching the whole output |
| File names, dates, sizes, descriptions, directory counts and current date | **volatile/runtime or fixture-derived**; assert identity/derivation, not the captured literal outside the seeded smoke |
| ANSI sequences and AquaScan ornament bytes (`b8`, `f8`, `a4`, `b0`, `ac`, `af`) | existing D2 wire surface; Latin-1 code points must remain valid target UTF-8. D10 introduces no high-bit literal |
| Selected repaint marker `[X]` | ASCII stable-const, existing NextScan surface; selection identity is runtime-derived |

This is an **interactive pager/hotkey surface**. The `More?` choice acts on a
lone key, while the number/name sub-prompts are line reads in the harness. Stage
5 must run the optional operator terminal glance required by §10.7; token-only
logs cannot establish local per-keystroke echo.

## Edge-probe status for the completeness critic

The table records facts, including gaps; it does not manufacture door behaviour
from `express.e`.

| Battery item | Status |
|---|---|
| Empty input | **captured** at both numeric and name line prompts (line 812); cancels selection and returns to `More?` |
| Whitespace-only input | **captured** at number and name prompts; CR submits an empty/no-op entry (`edge_reprobe.txt:206,304`) |
| Bare CR vs bare LF | **captured** at both prompts: CR submits; LF emits/echoes an LF byte but does not submit, so the harness recovered with CR (`edge_reprobe.txt:206-209,304-307`) |
| Out-of-range number | **captured**: `999`, silent ignore (line 812) |
| Non-numeric number | **captured**: `abc`, silent ignore (line 812) |
| Unknown token / sub-verb | unknown name **captured and accepted**; unknown pager hotkey **not captured** |
| Trailing junk | **captured**: numeric `1 garbage` accepts whitespace token `1` and ignores junk token `garbage`; name trailing spaces are trimmed (lines 812, 847) |
| Lone key vs full line | **captured**: pager `R`/`F` are lone hotkeys; a partial number/name echoed during idle but did not submit until CR (`edge_reprobe.txt:206-209,304-307`) |
| Sub-prompt accept/reject set | relevant `R`, `F`, `L`, `Q`, numeric/name inputs captured; the full AquaScan pager alphabet and off-list reject are outside D10 and **not exhaustively captured** |
| Empty-collection gate | **captured** for `F H`: `Nothing found!`; internal `A`: `No file flags` |
| Enter default | empty numeric/name inputs cancel; no value default exists at these prompts |
| Volatile Enter default | `N` date was supplied explicitly; the last-call default is earlier D9 scope, not D10 |
| Case sensitivity | lower-case pager `r`/`f` and lower-case names captured; normalized equivalently |
| Plural / duplicate flag entries | **captured**: numeric `1 2 1` selects distinct #1/#2 once; name input is one whole normalized line, including spaces/repetition (`edge_reprobe.txt:206-244,304-342`; source boundary `express.e:12638`) |
| Inline forms | `F A`, `FR A`, `F 2`, `F H`, bare `N` captured; D10 changes the pager registry, not the already-specified top-level directory grammar |

## Explicit limitations carried into Stage 3

- No duplicate normalized file name exists across the two seeded directories,
  so exact duplicate identity is unobserved and stays with item 30 / D2s.
- Changed-data reload is captured by the bounded E3 retry; it replaces the old
  numeric mapping (`edge_reprobe.txt:377-466`).
- The original held catalogue was empty. A single bounded populated-HOLD retry
  timed out after `R` and its partial block was not emitted; per §10.8 the
  campaign was not repeated. Allium requires rejection
  (`specs/files.allium:187-203`), while legacy name flagging performs no status
  lookup (`express.e:12523-12542`); the numeric HOLD outcome remains unresolved
  for the human gate.
- AquaScan's implementation source is unavailable. Any uncaptured door-pager
  edge must be re-probed within the board connection budget or explicitly left
  unresolved at the human gate; internal `express.e` cannot prove AquaScan's
  missing wire/control-flow facet.
