# Design: NextScan interactive fixes — UTF-8 wire, D2b hotkeys, [X] flag marker

Status: approved 2026-06-12 (Paul). Companion to `designs/NEXTSCAN.md`; where the
two disagree, this document wins — it amends the D2 decisions that produced the
three defects diagnosed in the 2026-06-11 root-cause analysis.

## 1. Context

Slice D2 shipped three user-visible defects, all *oracle defects* — the
capture-replay test suite faithfully defended decisions made while reading
flat transcripts:

1. **Dead pager prompts.** More?/ns-confirm/flag prompts read whole lines with
   `TerminalEcho::Silent` (`file_list/mod.rs:289`); the server negotiates
   `WILL ECHO` so the client never local-echoes — typing is invisible until
   Enter. The line-granularity lone-`n` hold then demands a *second* complete
   line (`file_list/mod.rs:300-305`), contradicting `NEXTSCAN.md:55`
   (line `n` → Quit). The real door is single-keypress with immediate echo
   (proven by bare-byte probes: `ae_tierd_aquascan3.txt` S2,
   `ae_tierd_aquascan4.txt` U1).
2. **Mojibake.** `file_list/wire.rs:26,56-61,163-164` emit raw Latin-1 bytes
   (`\xb8 \xf8 \xa4 \xb0 \xac \xaf \xa9`) for capture parity; UTF-8 terminals
   render each as U+FFFD. Meanwhile `wire_text.rs:88-89,112,115` emit `©` as
   UTF-8 — one session mixes encodings, so no client renders everything.
3. **Silent flagging.** Flag input is read and discarded
   (`file_list/mod.rs:331-336`, deferred to slice D5); the immediate silence is
   door-faithful, but the legacy's downstream feedback (logon
   `** Flagged File(s) Exist **`, logoff `checkFlagged` warning, AutoSaving
   banner) is wholly absent and there is no visual mark on rows.

Decisions (Paul, 2026-06-12): pull D2b (true hotkeys) forward and prove it
fixes the symptom; make the wire UTF-8 everywhere; defer the downstream flag
surfaces to a future slice but add an on-row `[X]` marker now, painted in
place when a visible row is flagged.

## 2. Build order

Three slices, each TDD + `cargo mutants` per AGENTS.md, in this order because
each re-pins bytes the next depends on:

| Slice | Name | Delivers |
|-------|------|----------|
| D2u | UTF-8 wire policy | re-encoded constants, char-aware columns, encoding gates, policy docs |
| D2b | True hotkeys | `Terminal::read_key`, hotkey More?/ns-confirm, echoing flag reads |
| D2f | Flag marker | session `FlaggedFiles`, row marker slot, in-place repaint |

A probe session against the FS-UAE reference board precedes D2b (§6.1).

## 3. Slice D2u — UTF-8 wire policy

**Policy (normative, lands in AGENTS.md):** the NextExpress wire is valid
UTF-8. Legacy Latin-1 bytes are re-encoded to the same code points in UTF-8.
Every departure from captured bytes gets a COMMAND_PARITY.md row. This
reconciles the contradiction between SLICES.md:303-309 (re-encode via `\u{}`)
and NEXTSCAN.md:11 (raw `&[u8]`); NEXTSCAN.md:11/:184 are corrected.

**Code changes:**
- `file_list/wire.rs`: `HELP_BANNER` (:26), `SEPARATOR_ART_A` (:56-57),
  `SEPARATOR_ART_B` (:60-61), `HELP_SCREEN` (:163-164) become `&str`
  constants carrying the real glyphs — `\xb8`→`¸`, `\xf8`→`ø`, `\xa4`→`¤`,
  `\xb0`→`°`, `\xac`→`¬`, `\xaf`→`¯`, `\xa9`→`©`. The wave separator renders
  as `_¸,ø*¤°¬°¤*ø,¸_¸,ø*¤°¬¬°¤*ø,¸_` (line A) /
  `¸,ø*¤°¬¯¬°¤*ø,¸_¸,ø*¤°¬°¤*ø, MM-DD-YY` (line B).
- `wire.rs` visible-column accounting (`visible_columns`, ~:273) counts
  `char`s, not bytes (all affected glyphs are single-cell Latin-1 code
  points; no grapheme handling needed). The 79-visible-column banner maths
  must hold for multi-byte UTF-8.
- Test expectations re-pin mechanically: captured Latin-1 bytes transcoded to
  UTF-8 in the expectation literals.

**Gates:**
- Unit test sweeping every wire constant in the crate through
  `str::from_utf8`/type-level `&str`.
- The e2e telnet smokes assert the *entire* captured session stream decodes
  as UTF-8 (this is the regression gate for all future slices).

**Docs:** COMMAND_PARITY.md gains rows for art/`©` re-encoding (deliberate
policy departure, legacy byte vs ours); the legend is amended so encoding and
interaction divergences can never be tagged COSMETIC; the Tier A–C `©` rows
(:78, :204) are resolved by the policy.

## 4. Slice D2b — true hotkeys

**New port:** `Terminal::read_key() -> KeyEvent` with
`KeyEvent { Char(char), Enter, Other }` (exact shape may grow during TDD, but
stays this small unless a test forces more):
- Telnet adapter feeds it byte-at-a-time, IAC-aware.
- `CR`, `CR LF`, `CR NUL` normalise to one `Enter`. Bare `LF` also maps to
  `Enter` (inference for line-mode clients — probe P2 in §6.1 confirms or
  corrects).
- `ESC [ … final-byte` sequences are swallowed as ONE `Other` event so an
  arrow key cannot fire three verbs; a lone `ESC` is `Other`.

**More? prompt becomes a hotkey loop** (replaces the Silent line read),
matching the captured door byte-for-byte:
- `Y`/`y`, Enter, Space, `Other`, unknown chars → continue (69-space
  overprint; captured "unknown keys continue" door default).
- `C`/`c` → form feed, counter reset.
- `Q`/`q` → `Quit\r\n` + exit tail.
- `?` → in-pager pause help + full page redraw (existing D2 bytes).
- `F`/`f`, `R`/`r` → flag prompts (line reads, §below).
- `n`/`N` → echo `n` immediately, hold. Next key: `s`/`S` → prompt line wipe
  + non-stop confirm; **Enter → Quit** (uncaptured corner — in-pager help
  pins `N … Quit`; probe P1 verifies before this is hard-coded); any other
  key → `BS SP BS` erasing the held `n`, then that key's verb runs
  (captured, U1).
- Case-insensitivity is assumed door-wide (only `Q/Y` upper and `n/ns` lower
  were captured) and recorded as inference in COMMAND_PARITY.md.

**ns-confirm** (`Non-stop scrolling! Are you sure (Y/n)?`) becomes a hotkey
read too (captured: harness sent bare single bytes).

**Flag and Directories prompts stay line reads** (line inputs on the real
board). Flag entry switches from Silent+handler-replay to a `read_key`-based
line collector: each printable char echoes as it arrives, Backspace echoes
`BS SP BS`, Enter finishes WITHOUT emitting `\r\n` — preserving the captured
wire shape (entry echoed, no trailing CRLF, then `\r`+79sp+`\r`) while making
interactive typing visible. The Directories prompt keeps its existing
`Visible` line read (its captured echo includes `\r\n`).

**Cleanup:** `TerminalEcho::Silent` should have no remaining users — delete
the variant, its `EchoMode` plumbing, and the b4a09a0 silence-pinning tests.
COMMAND_PARITY.md:724 ("Enter-required pager keys") is replaced by the
restored-parity row; NEXTSCAN.md:51/:55 are updated to describe the hotkey
implementation as landed.

**Outcome at the user's terminal:** pressing `n` at More? echoes instantly
and (after Enter, per the held-n rule — or instantly for `Q`) acts with no
extra keystrokes; `Q` quits on a single unechoed-until-now keypress. The
dead-prompt and double-Enter symptoms are gone.

## 5. Slice D2f — `[X]` flag marker

**State:** `FlaggedFiles` — a session-scoped set of file identities
(conference, area, name) in the domain, mutated by the More? `F`/`R` verbs.
This is the explicit precursor to slice D5 persistence; no disk writes here.

**Entry parsing:** `F` takes whitespace-separated file names, matched
case-insensitively against the files listed in the current scan; `R` takes
`[ File #N ]` counter numbers (the lister keeps a number→file map per scan).
Entries that match nothing are silently ignored (door-faithful; the absent
repaint is itself the signal). No unflag verb — the legacy `A` (alter flags)
verb stays advertised-but-inert until D5.

**Row rendering:** every file row gains a 4-character marker slot between the
13-char padded name field and the check char: `[X] ` when flagged, four
spaces otherwise — check/size/date/description columns all shift right by 4
but stay mutually aligned; description wrap columns recompute. Over-long
(≥13-char) filenames, which already render unaligned, append ` [X]` after the
name when flagged. The marker introduces no new SGR — it renders in whatever
colour is active at that point of the row. This is a deliberate NextExpress
departure from the captured row, recorded in COMMAND_PARITY.md.

**In-place repaint:** the pager already materialises each dir's rendered
lines; it additionally tracks which screen line each file row currently
occupies on the page (offset = lines emitted since that row, prompts
included). After a successful flag, for each newly flagged row still on the
current page: `\r`, cursor up N (`ESC[NA`), cursor to the marker column
(`ESC[<col>G`), write `[X]`, `\r`, cursor down N (`ESC[NB`), redraw More?.
Rows that scrolled off earlier pages simply show `[X]` at their next render
(later page, `?` redraw, or a fresh `F` run).

**Non-ANSI sessions:** `ColourTerminal` strips only SGR; cursor CSI sequences
would garble a non-ANSI client. Repaint is suppressed when ANSI is off
(capability exposed to the handler); those sessions get the marker at next
render.

## 6. Verification — the fix must demonstrably fix it

### 6.1 Probe battery (before D2b lands)

Boot the FS-UAE reference board (per the Docker harness notes) and run a new
`comparison/harness/ae_tierd_probes.py` recording exact bytes sent/received
per step:
- **P1**: at More?, send bare `n`, idle-snapshot, then bare `\r` — pins
  held-`n` + Enter (design assumes Quit).
- **P2**: at a fresh More?, send bare `\n` — pins LF-as-keypress (design
  assumes Enter/continue).
- **P3**: at the flag prompt, send a filename one byte at a time with
  idle-snapshots — pins per-keystroke echo at door line reads.

Results land in `comparison/transcripts/` + `live-observations.md`; if the
board contradicts a design assumption, the board wins and this doc is
amended.

### 6.2 Keystroke-granular tests (the structurally missing shape)

New e2e smoke style: an in-process char-at-a-time client (per AGENTS.md §e2e,
in-process listener) that sends ONE byte, then asserts the echo/action bytes
arrive *before any terminator is sent* (read with timeout). Minimum coverage:
`n` echo-on-keypress; `Q` acts without Enter; flag-entry chars echo as typed;
repaint sequence after flagging a visible row.

### 6.3 Type-at-it acceptance

After green + mutants: boot the server, drive a raw char-at-a-time session
end-to-end; then Paul telnets in and exercises `F` by hand before the work is
called done. AGENTS.md's Before-Committing checklist gains item 6 making this
mandatory for user-facing slices.

## 7. Error handling

- `read_key` on EOF/disconnect → session-ending error, same path as the line
  reads today.
- Flag entries that parse but match nothing: ignored silently. `R` with
  non-numeric tokens: ignored silently (door uncaptured; simplest rule).
- Repaint never runs with ANSI off or for rows not on the current page.

## 8. Future slices (recorded, not built now)

Added as named entries in NEXTSCAN.md §10 / `slices/cmds-files-list.md`:
- **D5 FlaggedFile persistence** + logon `** Flagged File(s) Exist **` + BEL
  (`express.e:2791-2794`, captured ×5), logoff `checkFlagged` warning
  (`express.e:12667-12673`), `** AutoSaving File Flags **` (`express.e:2803`),
  download integration.
- **A (alter flags)** door verb + stock `showFlags` surfaces.
- A fresh capture session for AquaScan's un-exercised `A`/`D` verbs.

## 9. Process docs touched

- AGENTS.md: wire-encoding policy; Before-Committing item 6 (type-at-it).
- COMMAND_PARITY.md: legend fix (interaction/encoding ≠ COSMETIC); new rows
  per §3/§4/§5; :724 replaced.
- NEXTSCAN.md: :11/:51/:55/:184 amended to match this doc.
- live-observations.md: probe results + a "methodology blind spots" section
  (echo timing, charset assumptions, effects-after-prompt).

## 10. Acceptance criteria

1. Typing at every NextScan prompt echoes per keystroke on a stock UTF-8
   telnet client; More?/ns-confirm act on single keys with no Enter.
2. No `�` anywhere in a session; separator art renders as the wave; all
   session bytes decode as UTF-8 (gated by test).
3. Flagging a visible file paints `[X]` into its row immediately; re-listing
   shows `[X]` on all flagged rows.
4. `cargo nextest run`, `cargo build` (no warnings), doctests, and
   `cargo mutants` clean per slice; SYSTEM.md updated.
5. Paul has typed at it.
