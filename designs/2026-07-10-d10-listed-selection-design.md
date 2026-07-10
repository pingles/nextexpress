# Design brief — Slice D10: NextScan current-directory selection index

**Date:** 2026-07-10. **Status:** A/A/A implementation and local verification
complete; the Stage-5 live dual-target comparison is deferred as a documented
follow-up.

## 1. Scope and ground truth

D10 corrects the identity behind the existing AquaScan `More?` pager's
`R <number>` and `F <name>` actions. It does not add a command, prompt, wire
literal, port, repository method, persistence rule, or transfer behaviour.
The implementation stays private to
`rust/src/app/menu_flow/file_list/scan.rs`.

The live evidence is:

- [`ae_tierd_d10_selection.txt`](../comparison/transcripts/ae_tierd_d10_selection.txt),
  driven by
  [`ae_tierd_d10_selection.py`](../comparison/harness/ae_tierd_d10_selection.py);
- the bounded edge retry
  [`ae_tierd_d10_edge_reprobe.txt`](../comparison/transcripts/ae_tierd_d10_edge_reprobe.txt),
  driven by
  [`ae_tierd_d10_edge_reprobe.py`](../comparison/harness/ae_tierd_d10_edge_reprobe.py);
- the indexed findings and capture limitations in
  [`d10-selection-live-observations.md`](../comparison/evidence-tierD/d10-selection-live-observations.md).

| Facet | Grounded result |
|---|---|
| Forward multi-directory | `F A` restarts numbering in each directory; directory 2's `R 1` selected `FRESHUPL.LHA`, not directory 1's #1 (`selection.txt:188-398`). |
| Reverse multi-directory | `FR A` walked directories 1 then 2, reversing rows inside each; directory 2's `R 1` selected `TOOLPACK.LHA` (`selection.txt:428-645`). |
| Empty transition | A date-filtered `N` found nothing in directory 1; directory 2's new #1 selected `MYDEMO.DMS` (`selection.txt:675-738`). |
| Reload | After Dir2 was replaced with Dir1, `L` rebuilt the display and `R 1` selected the new `ANSIPACK.LHA`, not stale `FRESHUPL.LHA` (`edge_reprobe.txt:377-466`). |
| Numeric grammar | Empty, whitespace, `999`, and `abc` are no-ops; `1 garbage` selects token `1`; `1 2 1` selects #1 then #2 once (`selection.txt:768-852`; `edge_reprobe.txt:181-244`). |
| Name grammar | The door accepts an unknown name and one whole space-containing line, trims the outside, uppercases it, and stores it once (`selection.txt:768-852`; `edge_reprobe.txt:279-342`). This agrees with `express.e:12523-12542,12638`. |
| HOLD | Empty `F H` is captured. The single bounded populated-HOLD retry timed out after `R`; no numeric-selection result or clean-logoff claim is made for it. |

The Stage-1 operator choice made board-as-shipped AquaScan authoritative for
numeric pager selection and kept D10 bounded. Stable duplicate-file identity
remains SYSTEM item 30 / D2s.

## 2. Candidate panel and refutation

The cleanest-seam designer emitted a candidate. The minimal-change and
closest-to-legacy designers did not emit after their one permitted retry, so
the independent judge reconstructed those two framings under hardening
§10.10 rather than spinning the pipeline.

| Rank | Candidate | Score / 25 | Disposition |
|---|---|---:|---|
| 1 | B — cleanest seam | 20 | Keep a named index, but move it from the proposed domain module into private pager state. |
| 2 | A — minimal change (judge-reconstructed) | 19 | Graft its local blast radius and absence of new ports/adapters. |
| 3 | C — closest legacy/live (judge-reconstructed) | 17 | Graft its whole-line grammar and its distinction between reversed rows and directory order, subject to the authority gate. |

Two independent refuters required these revisions:

1. Use `Vec<FlaggedKey>`, not `HashMap<u32, FlaggedKey>`: displayed numbers
   are dense `1..=N`, so direct checked `n - 1` indexing is simpler, ordered,
   O(1), and cannot represent accidental holes.
2. Register a visible numbered row only after both its row and CRLF writes
   succeed, but before a page-boundary `More?` is processed. Otherwise the row
   visible immediately above that prompt could not be selected.
3. Separate per-directory row reversal from directory traversal. The current
   `ScanKind::Full { reverse: true }` controls both (`scan.rs:76-113,190-194`)
   and is also used by `N R`; an `FR A` correction must not silently change
   the unprobed multi-directory `N R` path.
4. Stage keys, render from the staged keys, and mutate `FlaggedFiles` only
   after all terminal writes succeed. The current path mutates before its
   clear/repaint/`More?` writes (`scan.rs:560-563`).
5. State the complete input/lifecycle grammar and label Allium conformance as
   partial. Unchecked names cannot be described as Allium-safe merely because
   held rows are omitted from numeric lookup.

## 3. Data structure and lifecycle

`ScanState.listed: Vec<ListedRow>` is replaced by a private wrapper:

```rust
struct DisplayedSelectionIndex {
    keys: Vec<FlaggedKey>,
}
```

Its deliberately small API is:

- `begin_directory()` clears the prior mapping before a directory/HOLD fetch,
  including every reload and an empty result;
- `record(&ListedRow)` ignores unnumbered rows, asserts that the next framed
  number is dense, and pushes the key;
- `resolve(u32)` uses checked `n - 1` indexing and returns only a key from the
  current displayed directory.

The index is retained across page boundaries and `?` redraws within a
directory. It is populated incrementally after successful row output, never
from the eagerly assembled but not-yet-visible tail. It is cleared at the top
of the per-directory/reload loop, before refetching rows, so a failed or empty
reload cannot leave stale numeric identity.

`ScanState.page: Vec<ScanLine>` remains unchanged and separately owned. It is
ordered, bounded to the current page, and needed for redraw/cursor geometry;
it is not an identity lookup structure.

If the gate chooses unchecked legacy name input, `ScanState` also carries the
current conference needed to construct `FlaggedKey::new(conference, line)`.
Name input does not resolve through `DisplayedSelectionIndex`.

## 4. Ordering seam

`ScanKind::Full { reverse }` continues to mean **row order**. `ScanMode` gains
a private, explicit directory-order property (`Forward` or `Reverse`) used by
`walk_span`. This seam is forced by the captured `FR A`/source conflict; it is
not a port or speculative extension.

- `N R` retains its current/source-derived reverse directory walk regardless
  of the `FR A` decision.
- `FR` retains reverse rows. Its multi-directory walk follows the human gate.
- Forward `F` and filtered `N` modes retain their existing directory order.

## 5. Input and index grammar

| Surface/input | Authority | Intended handling |
|---|---|---|
| New directory, empty directory, HOLD, or `L` reload | capture: forward/empty/reload; HOLD numeric unresolved | Clear before fetch; replacement, never accumulation. |
| Numbered row | capture + display invariant | After row bytes and CRLF succeed, assert dense next number and record it before any resulting `More?`. |
| Continuation/plain row | existing framing | Do not register. |
| Page boundary / `?` redraw | capture/existing pager | Retain the current-directory index; retain `page` only for redraw geometry. |
| Pager `R` / `r` | capture | Existing lone hotkey opens `File number(s) to flag:`. |
| Empty or whitespace + CR | capture | No-op; redraw `More?`. |
| Bare LF | edge capture | Does not submit; no mutation until a later CR. Preserve the existing line-reader rule. |
| `1 2 1` | edge capture | Resolve strict decimal whitespace tokens in first-occurrence order; stage each distinct unflagged key once. |
| `1 garbage` | capture | Resolve `1`; silently ignore the separate invalid token. |
| `0`, `999`, `abc`, signed token | strict parser/current behaviour; `999`/`abc` captured | Silently ignore tokens that fail `u32` parsing or current-index lookup. |
| `1garbage` | unprobed | Strict-invalid no-op; mark PLAUSIBLE rather than claiming AquaScan parity. |
| Pager `F` / `f` | capture | Existing lone hotkey opens `File name(s) to flag:`. |
| Name empty/whitespace + CR | capture/source | No-op. |
| Name of length 1 after trim | `express.e:12532` | Ignore. |
| Name of length >1 | accepted A/A/A gate | Trim once, uppercase via `FlaggedKey`, preserve the whole line including internal spaces, and accept unknown names. This matches legacy and deliberately does not require an existing downloadable file. |
| Name repaint | existing marker surface | Repaint only when a staged key matches a current-page row; an unknown legacy name has nothing to repaint. |
| `L` | capture | Clear before refetch; register only newly emitted rows. |
| `Q`, `Y`, `C`, `K`, `?`, `n`/`ns`, unknown pager key | existing captured pager | Preserve wire and control flow; none clears the index except `L`. |
| EOF/idle/read failure before CR | hardening §10.4 | Abort/propagate through the existing model with no flag mutation. |

The existing input byte cap, backspace behaviour, CR echo choreography and
wire prompts are unchanged. D10 adds no accepted top-level command form.

## 6. Plan, render, commit

Replace mutating `apply_flags` with a pure planning step that accepts the
entry, selection mode, current conference/index, and an immutable
`FlaggedFiles`. It returns distinct, previously unflagged keys in input order.
Numeric planning uses `DisplayedSelectionIndex::resolve`; name planning uses
the gate-selected policy. A small local set may suppress repeated staged keys
while the returned `Vec` preserves first-occurrence repaint order.

The pager then:

1. collects the line through the existing reader;
2. plans keys without mutation;
3. writes the flag-prompt overprint clear;
4. renders repaint sequences from the staged keys;
5. writes the restored `More?` prompt;
6. commits the staged keys to the in-memory `FlaggedFiles` set.

The commit is infallible after the final terminal write. Any read/write failure
before it leaves the set unchanged. Row registration has a different timing:
it occurs after the row's two successful writes and before `More?`, because
that row is already visible and must be immediately selectable.

## 7. Allium and legacy boundaries

`files.allium:164-185` defines ordinary listing-visible statuses as
`{available, lcfiles}`. `FlagFile` additionally requires an existing file,
download access, an allowed status, no duplicate, capacity, and `flagged_at`
creation (`files.allium:187-203`). D10 can make numeric identity correspond to
the currently emitted ordinary row and preserves duplicate idempotence through
`FlaggedFiles`, but it does **not** complete the Allium rule:

- unchecked captured names deliberately permit a non-existent file;
- `Right::Download`, stable `FileId`, the 1000-entry cap, and `flagged_at` stay
  with SYSTEM items 24, 30, and 32;
- populated-HOLD numeric behaviour remains uncaptured; the accepted
  compatibility extrapolation registers those rows even though Allium rejects
  held-file flags;
- accepting HOLD numbers is consistent with unchecked legacy name entry, which
  can also name a held or non-existent file.

The chosen departures must be recorded in `COMMAND_PARITY.md`; no design
choice is labelled full Allium conformance.

## 8. Ordered TDD plan

1. Failing pure tests for dense `DisplayedSelectionIndex` lookup, zero/range
   rejection, clear/replacement, and same-directory retention; implement the
   private wrapper.
2. Failing emission tests for row-write failure, CRLF-write failure, and the
   page-boundary registration-before-`More?` invariant; move registration.
3. Failing flow tests for `F A` current-directory replacement, an empty
   transition, a changed-data `L`, and multi-page same-directory lookup.
4. Failing table tests for numeric grammar: blank, whitespace, `1 2 1`,
   `1 garbage`, `0`, `999`, `abc`, signed, and strict-invalid `1garbage`.
5. Gate-dependent failing name tests: trim/case, one-character rejection,
   unknown name, and one preserved space-containing line—or the Allium-resolved
   alternative.
6. Failing adapter tests at every flag-path read/write boundary: input echo,
   overprint, each repaint write, and final `More?`; assert no staged flag was
   committed. EOF/idle/read failure likewise leaves the set unchanged.
7. Failing ordering tests that pin the chosen `FR A` walk and independently
   pin unchanged `N R` reverse directory traversal.
8. Failing HOLD test for the chosen numeric policy, explicitly separate from
   the name policy.
9. Run `cargo nextest run`, `cargo build`, `cargo test --doc`, clippy with
   warnings denied, and `make mutants-diff DIFF_BASE=main`. These local gates
   completed successfully. The Stage-5 live NextExpress/reference comparison
   remains the follow-up described in §10.

## 9. Human authority gate — accepted A/A/A

The independent design judge and the authority reconciler agree on the name
policy but disagree on the two uncaptured/conflicting facets. Hardening §10.3
makes `express.e` the default for a door/source conflict; a live-door choice is
still available but must be an explicit override.

| Decision | A | B | Assessments |
|---|---|---|---|
| **1. Name policy** | **Captured/source unchecked whole line:** trim, require length >1, uppercase, store once even if unknown; document the Allium departure. | Resolve an existing, downloadable file under Allium. | Judge and authority both recommend **A**; `express.e:12523-12542,12638` independently confirms the capture. |
| **2. `FR A` directory order** | **Source/current:** directories high→low, rows reversed; document divergence from live AquaScan. | **Live AquaScan:** directories low→high, rows reversed; explicitly override the source default. | Authority recommends/defaults to **A** under §10.3. The design judge recommends **B** because it is directly captured and the earlier Stage-1 choice favoured the shipped door for pager selection. Either choice leaves `N R` unchanged. |
| **3. Populated-HOLD numeric policy** | **Compatibility extrapolation:** register held row numbers and permit selection; document that the result is uncaptured and departs from Allium status rules. | **Allium-safe exclusion:** display held rows but do not register their numbers; document that live AquaScan is unresolved. | Authority recommends **A** for consistency with unchecked legacy names. The design judge recommends **B** because no numeric result was captured and Allium rejects held flags. |

The operator chose **A/A/A** on 2026-07-10, asking D10 to match legacy
behaviour:

1. Name entry preserves the captured and source-backed unchecked whole line.
2. `FR A` keeps the current/source high→low directory order; the live
   low→high AquaScan result remains a documented divergence.
3. Populated-HOLD numbers remain selectable. This is labelled an uncaptured
   compatibility extrapolation and an Allium status departure, not a live
   parity claim.

`COMMAND_PARITY.md` records the authority and captured/source/extrapolated
status of each choice. `N R` remains unchanged.

Section 9 is updated with the operator's decisions before Stage 4. The same
decisions then update `COMMAND_PARITY.md`, `SLICES.md`, and the D10 slice row.

## 10. Close-out verification and deferred live comparison

The implementation reached a clean merge point on 2026-07-10:

- `cargo nextest run`: 1263 passed;
- `cargo build`, `cargo build --release`, `cargo test --doc`, and
  `cargo clippy -- -D warnings`: passed; and
- `make mutants-diff DIFF_BASE=main`: 29 mutants considered, 24 caught,
  5 unviable, 0 missed.

The operator declined the optional §10.7 hands-on terminal glance and asked to
wrap up at this clean point. Stage 5's independent live NextExpress/reference
cross-comparison is therefore deliberately deferred, not silently treated as
complete. Its prepared four-session set covers:

1. directory-local identity across `F A`, `FR A`, filtered `N`, and empty HOLD;
2. page retention, redraw/skip/reload, and the remaining pager verbs;
3. complete numeric grammar, including the explicitly provisional
   `1garbage`; and
4. complete whole-line name grammar and repaint behavior.

When resumed, run those sessions character-at-a-time against the isolated
NextExpress and FS-UAE targets, cross-mark both transcripts, run the Stage-5
completeness critic, and close each legacy session with `G Y`. No further code
change is currently known to be required.

## 11. Explicitly out of scope

- Stable duplicate file identity (`FileId`), same-named files in different
  areas, and area-aware persistence (SYSTEM item 30 / D2s).
- Flag capacity, insertion-ordered persistence, command-time saves,
  active-session purge, download authorization, transfer preflight, and
  `flagged_at` (SYSTEM items 24, 30, and 32).
- New wire literals, encoding changes, pager verbs, top-level command grammar,
  repository methods, or broad engine extraction.
- Re-probing populated HOLD in D10; the bounded completeness loop is exhausted.
