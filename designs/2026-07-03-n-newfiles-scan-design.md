# Design brief — Slice D9: item-17 NextScan engine extraction + the `N` (new-files scan) command

**Date:** 2026-07-03. **Target file:** `designs/2026-07-03-n-newfiles-scan-design.md`.
**Ground-truth status:** the fresh live capture **landed**: `comparison/transcripts/ae_tierd_newfiles.txt` (175,117 bytes, 2026-07-03 21:33, two full logon rounds, ~30 probes, sections N1–N9 plus a second clean pass). This brief is written against it. It **refutes two claims in the D9 slice text** (`slices/cmds-files-list.md:441-463`): `N` does *not* walk multiple conferences and does *not* consult the CF file-scan flag.

---

## 1. Ground truth and TO-CONFIRM list

### 1.1 What the capture establishes (anchors)

| Surface | Anchor | Finding |
|---|---|---|
| Entry preamble | N1a | echo `N\r\n`, then `\x1b[0m\r\n`, AquaScan banner (`'n ?' for options` label, 15-dash centre run, 77 visible cols), blank — same shape as F |
| Date prompt | N1a | `\x1b[36mDate: \x1b[32m(\x1b[33mMM-DD-YY\x1b[32m)\x1b[36m, \x1b[32m(\x1b[33m-X\x1b[32m) \x1b[36mDays, \x1b[32m(\x1b[33mR\x1b[32m)\x1b[36meverse, \x1b[32m(\x1b[33mEnter\x1b[32m)\x1b[36m=<mm-dd-yy> ?\x1b[0m ` (trailing space inferred through harness markers — confirm in the manual type-at-it pass) |
| Default date | round 1 = `06-25-26` while today was 07-03; round 2 = `07-03-26` after round-1 logoff | default = **day of previous call** (door help: "Scan since day of last call"). Matches NextExpress `user.last_call()`, which mutates only at session finalise (`rust/src/domain/session/lifecycle.rs:132`, `rust/src/app/session_flow.rs:313`). No new persisted field needed |
| Post-date flow | N1a/N1b/N2, N9 | the door's own `Directories: (1-N), (A)ll, (U)pload, (H)old, (Enter)=None ? ` prompt — **byte-identical to bare F's** (`file_list/wire.rs:411-416`); **current conference only** (N9 in conf 1 shows `(1-1)`); Enter=None aborts with `\r\n` + `\r\n\x1b[0m\r\n` (F's single-reset abort) |
| Scan header | N1b/N2/N3 | `Scanning dir {n} for {mm-dd-yy}... Ok! / Nothing found!` — plain, uncoloured |
| Reverse | N4, N4b, N7r | `R` at the prompt (optionally `R <date>`, date ignored) and inline `N R <dir>` run **exactly the FR mode**: `Reverse-scanning dir 2... Ok!`, full unfiltered dir, newest-first |
| Body | N2/N3/N7a | identical to F: wave-art date separators, `[ File #N ]` frames, plain fall-through rows, continuations, `[ End of File List ]` — **filtered set renumbered from #1** (N3: `-30` → PROTRACK.LHA is File #1). BADUPLD.LHA (check char `'F'`, status Available — `seed.rs:99`) **is listed** (File #19 in N2's pass-2 full scan) |
| Pager | N2/N7c | **base `MORE_PROMPT`** — zero `(S)kip Conf` occurrences anywhere; `Q` → `Quit\r\n` + two-reset tail; `Y` at post-End → 69-space overprint + inter-dir CRLF; empty dirs run header-into-header, no blank/More? |
| Page-1 boundary, prompt path | N2 (counted) | first More? after **exactly 29 counted lines starting at the post-answer blank** — the door **resets its counter at each interactive prompt** |
| Page-1 boundary, inline path | N7c (counted) | first More? after **29 counted lines starting at the reset line** (reset + banner + blank + header + blank + 24 rows) — F's span-path model exactly |
| Pages 2+ | N2 (segments 30/27/29/29/13) | door drift — inherit F's documented COSMETIC divergence (NextScan pages uniformly at 29) |
| Errors | N5, N8b, N7e | `Error in date!`: echo, `\r\n`, literal, `\r\n\r\n\x1b[0m\r\n` — **single-shot, exits to menu** (the internal's looping prompt is diff-record only). Out-of-range dir at the prompt: F's `The highest directory number is 2!` envelope byte-identical. `N R -1` → Copyright help banner + `Argument error! Type 'n ?' for help.` (F's argument-error envelope with `'n ?'`) |
| Help | N6 | `N ?` → `\x1b[0m\x0c\r\n` + Copyright banner + 2 blanks + 10 N-syntax lines + ASCII diagram + F's exact epilogue — same skeleton as F's `HELP_SCREEN` (`wire.rs:230-253`) |
| Inline grammar | N7a–N7r | `N <mm-dd-yy>` (dir defaults **Upload**), `N -30` (→ `06-03-26`, month underflow), `N T` (today), `N Y` (yesterday, `07-02-26`), `N S` (since last call), `N 2` (bare digit = **dir**), `N !2` → `Scanning dir 2 for the last 2 files... Ok!` (newest-2, ascending), `N <date> <dir>`, `N <date> <dir> Q` (quick: continuation lines dropped), `N <date> <dir> NS` (non-stop, no More?, same two-reset tail) |
| Exit tails | everywhere | every listing-shaped exit = F's `LISTING_EXIT_TAIL` (`\x1b[0m\r\n\x1b[0m\r\n`, `file_list/mod.rs:27-30`); aborts/errors = F's single-reset envelopes (`mod.rs:145`, `:159`) |
| Date boundary | SCAN sibling (`ae_tierd_aquascan.txt:421-428`) + `express.e:27976-27986` (`ddt>=day`) | filter is **inclusive**: `uploaded_at >= cutoff` |
| CF flag | capture + `express.e:591-608`, `:28066-28115` | `checkFileConfScan` gates **only** the logon `confScan` (which runs `runSysCommand('N','S U')` per flagged conference); menu N never consults it |

The seeded demo catalogue (`rust/src/app/seed.rs:79-134`) mirrors the FS-UAE fixture row-for-row (verified: ANSIPACK/TERMV48/PROTRACK/README1ST/FRESHUPL/MYDEMO/TOOLPACK names, sizes, dates match the capture), so capture literals drop straight into unit tests and smokes.

### 1.2 Consolidated TO-CONFIRM list (residual — needs a follow-up probe session or explicit ruling; each gets a PLAUSIBLE row in COMMAND_PARITY.md until settled)

1. **`H` at N's Directories prompt / inline `N <date> H`** — the hold dir under a date scan was never probed; header wording unknown. Shipped interim behaviour is defined in §3.4 (no `todo!()`/panic on this reachable, advertised input).
2. **`T`/`Y`/`S`/`!x` typed AT the date prompt** — only date/`-30`/`R`/`R <date>`/Enter/junk were probed there. Provisional: `Error in date!`.
3. **Junk answer at N's Directories prompt** — provisional: F's `Error in input!` envelope (same door machinery, byte-identical prompt).
4. **Inline letter spans `N <date> A` / `N <date> U`** — only numeric dirs and the default-Upload form captured; implementing via the shared span-token resolver is inferred from the help diagram.
5. **Bare `N <dir>` date source (N7d `N 2`)** — capture cannot distinguish SinceLastCall vs Today (pass-2 last call = today). Provisional: SinceLastCall per the help grammar `N [S] [dir]`; the diverging-clock test pins a *NextExpress choice*.
6. **`N mm-dd` with year omitted** (help-advertised `mm-dd[-yy]`) — year-defaulting and header label unverified. Provisional: current year from the Clock.
7. **Date-content validation strictness** — `99-99-99` / `13-40-26` (calendar-invalid but date-shaped) unprobed; internal accepts any 8 chars (`MiscFuncs.e:388-401`). Provisional: reject → `Error in date!` (prompt) / `Invalid` (inline); malformed-dashed-vs-junk inline split unprobed.
8. **`!1` singular/plural header, `!x` > dir size, `!x` on an empty dir, `Q` combined with `R`** — unprobed edges of captured features.
9. **Pager verbs beyond `Y`/`Q` at an N More?** (`C`, `F`/`R` flag, `?`, held-`n`/`ns`, stray `S`) — engine-shared on the real door too, but never exercised inside an N scan; pass-1 N10 fizzled, pass 2 has no N10.
10. **Inline out-of-range dir (`N 9`)** — prompt-path variant captured byte-identical to F (N8b); inline variant PLAUSIBLE.
11. **Trailing junk after a valid date at the prompt** (e.g. `06-01-26 X`) — `R <date>` shows extra tokens tolerated in that one form; general rule unprobed.
12. **No-prior-call default** (fresh user, `last_call == None`) — not capturable; NextExpress choice: today. Record it.
13. **Date-prompt echo discipline** (per-keystroke echo, backspace, trailing-space final byte) — AGENTS.md step-6 type-at-a-real-terminal item, plus the like-for-like FS-UAE pass (parent task #5).

---

## 2. Engine extraction design (item 17 — phase A, zero wire change)

### 2.1 Module layout — all under `file_list/`, zero visibility widening

Children of `file_list` see `wire.rs`'s `pub(super)` items via `super::wire::…` (visibility is subtree-closed), so siblings under `file_list/` need **no** visibility changes. `menu_flow/nextscan.rs` is rejected: it would force widening `file_list/wire.rs` items (or moving `wire.rs`/`dir_row.rs` with `pub(in crate::app::menu_flow)` adjustments) for no benefit. `menu_flow/pager.rs` (the *message* pager, `pager.rs:1-20`) is untouched — no collision.

```
rust/src/app/menu_flow/file_list/
  mod.rs            F entry points only: handle_file_list, file_list_prompt,
                    file_list_span, file_list_argument_error, the three repo read
                    helpers + empty_on_error, and (this slice) the zippy block.
                    Shrinks ~921 → ~500 lines.
  scan.rs           THE ENGINE: ScanFlow, ScanState, ScanMode/ScanKind, ScanLine,
                    ListedRow (moved from wire.rs — machinery, not bytes),
                    run_span, post_end_pause, stream_dir_body, emit_scan_line,
                    scan_more_prompt, repaint_flagged_rows, read_flag_entry,
                    finish_listing, PAGE_LINES, LISTING_EXIT_TAIL, apply_flags,
                    one parameterised overprint_clear(width) + two named wrappers.
  scan/tests.rs     the ~28 moved engine tests (§2.4).
  new_files.rs      N entry (phase B, §3) + pure date fns.
  new_files/tests.rs
  wire.rs           unchanged strata + a "Slice D9: the N (new-files) door wire"
                    section (zippy-section precedent, wire.rs:418-426);
                    assemble_dir_lines gains `quick: bool`.
  dir_row.rs        + pub(super) fn format_dir_date(SystemTime) -> String
                    (reuses DIR_DATE, MiscFuncs.e:278 FORMAT_USA — one source
                    for the mm-dd-yy shape, header and rows can never disagree).
  tests.rs          remaining F-entry (~12) + zippy (19) tests.
```

Engine items stay methods on `MenuFlow<'_, T>` in a second `impl` block in `scan.rs` (impl blocks split freely across files). Deliberately **not** a standalone engine struct: the flows interleave engine emits with `prompt_line` (which needs `&mut self` + `&mut MenuSession` for the `record_input` stamp, `menu_flow/mod.rs:830`), and a struct borrowing `&mut T` out of `MenuFlow` would deadlock that interleaving. Keeping `&mut ScanState`/`&mut FlaggedFiles` as per-call parameters is the borrow-honest shape (the existing scoped-borrow comments at `file_list/mod.rs:113-128`, `:166-168` document why).

### 2.2 The generalisation: `ScanMode`, not caller-supplied rows

Item 17's letter says "caller supplies the per-dir row set and header bytes"; closure-based suppliers fight the borrow checker (`&mut self` engine call vs `&self.services` capture) and eager pre-fetch loses quit-stops-fetching. The honest second-consumer generalisation is a mode enum owned by the engine — the caller picks the mode, the engine keeps lazy per-dir acquisition:

```rust
/// What a NextScan walk lists per directory and how it titles it.
pub(super) enum ScanKind {
    /// F / FR / N's `R` answer: the full listing, forward or newest-first.
    Full { reverse: bool },
    /// N: files uploaded on/after `cutoff` (inclusive, express.e:27976-27986);
    /// `label` is the mm-dd-yy the header echoes — derived once from the same
    /// date via dir_row::format_dir_date, never recomputed.
    NewSince { cutoff: SystemTime, label: String },
    /// N `!x`: the x newest files, ascending (capture N7n: MYDEMO then TOOLPACK).
    NewestLast { count: u32 },
}

pub(super) struct ScanMode {
    pub kind: ScanKind,
    /// N's `Q` token: drop description continuation lines (capture N7q).
    pub quick: bool,
}

async fn run_span(
    &mut self,
    state: &mut ScanState,
    conference: u32,
    span: FileSpan,
    areas: &[FileArea],
    flagged: &mut FlaggedFiles,
    mode: &ScanMode,           // replaces `reverse: bool`
) -> Result<(), T::Error>
```

Inside the per-dir loop, exactly two dispatch points change:

- **rows**: `Full{reverse}` → `self.files_in_area(…)` (+ reverse); `NewSince{cutoff,..}` → new helper `self.new_files_in_area(area_ref, cutoff)` over the new port method, same `empty_on_error` policy (`mod.rs:37-40`); `NewestLast{count}` → **saturating** tail of `files_in_area` (`let keep = files.len().saturating_sub(count as usize); files.split_off(keep)` — a plain `len - count` panics on `N !999`, TO-CONFIRM #8 territory), ascending kept.
- **header**: `Full` → existing `wire::scanning_dir_header(dir, found, reverse)`; `NewSince` → new `wire::scanning_new_header(dir, label, found)`; `NewestLast` → new `wire::scanning_newest_header(dir, count, found)`.

`stream_dir_body` passes `mode.quick` into `assemble_dir_lines(files, conference, flagged, quick)`. The Hold branch stays `Full`-only for F (captured bytes); N + Hold interim behaviour is §3.4. F's **two** call sites (`file_list/mod.rs:169`, `:215` — verified, not three) change to `&ScanMode { kind: ScanKind::Full { reverse }, quick: false }` — zero wire change; every existing byte pin must stay green untouched.

Restructure while moving: the per-dir walk becomes an inner fn returning `Result<ScanFlow, T::Error>`; **one** wrapper appends `finish_listing()`, collapsing the ~10 `== ScanFlow::Quit → finish_listing` sites (capture confirms N shares the two-reset tail, so the tail stays engine-fixed). A `begin_listing(&mut state, banner, flagged)` helper folds the duplicated counted-preamble loop (`mod.rs:117-127` vs `:206-214`).

The multi-conference section layer (SCAN/NSU `Conference: [ … ]` banners, `(S)kip Conf` verb, logon `confScan`'s plainer `checkForPause` pager, non-interactive `N S U` entry) is a **deferred seam** — a future `run_walk` wrapper per conference plus a `ScanFlow::SkipGroup` extension. Not built: the capture proves N doesn't need it. Recorded in SYSTEM.md.

### 2.3 One shared A/U/H/digit span-token resolver

`pub(crate) fn parse_span_token(token: &str) -> Option<FileSpan>` in `menu_command.rs` beside `val_prefix` (`menu_command.rs:286-301`): whole-token case-insensitive `A`/`U`/`H`, else leading-ascii-digit → `FileSpan::Dir(val_prefix(token))` (raw i64, range-checked by callers), else `None`. Four consumers, **each keeping its pinned error envelope and its own span expansion**:

- F parser (`menu_command/files.rs:48-58`): `None` → `FileListArg::Invalid` (`Argument error! Type 'f ?' for help.`)
- F Directories prompt (`file_list/mod.rs:148-161`): `None` → `Error in input!` + single-reset tail
- `resolve_zippy_span` (`file_list/mod.rs:858-877`): keeps its dense `1..=max` expansion + internal range check → `No such directory.` (F's `All` expands via catalogue area numbers, `mod.rs:241` — divergent on sparse sets; expansion never enters the resolver)
- NEW: N's parser dir token and N's Directories-prompt answer.

### 2.4 Test migration (byte expectations frozen; only paths move)

- Hoist to `menu_flow/test_support.rs` (the documented consolidation home, `test_support.rs:1-6`): `CaptureTerminal` (the file_list superset with the `ansi` flag — supersedes the near-duplicate in `menu_flow/tests.rs`), `menu_session()`, `test_user()`, `key()`/`keyed_terminal` helpers, plus shared fixtures `f_1_emitted_lines`/`area_lines`/`joined`/`services_with_demo_catalogue`/`EXIT_TAIL`.
- Move the ~28 engine tests to `scan/tests.rs` — **partitioned by content, not by line range** (they interleave with F-entry tests from :437 to ~:1855): More? verbs Q/Y/C/?/unknown/Enter, held-`n` pair, ns-confirm pair, flag prompts/`apply_flags`/persistence, repaint suite, page boundary, dir transitions (:1398/:1442), post-End More? (:1360), hold pair (:1651/:1829), empty-dir (:1740), failing-repo (:1769), the three NS streaming tests (:285/:322/:362). The ~12 F-entry tests (:223, :245, :267, :1500–:1719 prompt/help block) and the 19 zippy tests (:1857–:2306) stay in `file_list/tests.rs`.
- **Moved tests keep driving `handle_file_list`** (it is `pub(super)` in file_list ⇒ visible in `scan::tests`): every `lines[..29]` page pin, `up = 29 - index` repaint pin and `?`-redraw pin stays valid verbatim; only `super::` paths are rewritten. New engine-*API* tests for the generalisation drive `run_span` directly.
- `rust/tests/tierd_file_list_smoke.rs` (19 tests, independently restated literals) does not move and **must not change** during phase A — it is the drift gate.

---

## 3. N command design (phase B, capture-pinned)

### 3.1 Parser — `MenuCommand::NewFilesScan(NewFilesArg)`

`menu_command/files.rs::parse_new_files_command`, chained after `parse_file_list_command`; exact head token `N` case-insensitive via `command_tokens` (`menu_command.rs:317-323` — the Z-vs-ZOOM precedent, so `NS`/`NSU` never bind).

```rust
pub(crate) enum NewFilesArg {
    Prompt,                 // bare N
    Help,                   // N ?  (extra tokens -> Invalid, F precedent)
    Scan(NewFilesSpec),     // inline forms (captured N7a-N7r)
    Invalid,                // -> Copyright banner + "Argument error! Type 'n ?' for help."
}
pub(crate) struct NewFilesSpec {
    pub request: ScanRequest,
    pub span: Option<FileSpan>, // None => Upload default (capture N7a)
    pub quick: bool,            // Q
    pub non_stop: bool,         // NS
}
pub(crate) enum ScanRequest {
    SinceLastCall,          // bare `N <dir>` (TO-CONFIRM #5), `N S`
    Today,                  // N T
    Yesterday,              // N Y
    DaysBack(u32),          // N -x
    Date { month: u8, day: u8, year: Option<u16> },  // N mm-dd[-yy]
    NewestLast(u32),        // N !x
    Reverse,                // N R — full listing, newest-first, date-less (N4/N7r)
}
```

First-token classification: `?` alone → Help; `S`/`T`/`Y`/`R` (whole token, case-insensitive) → verbs; `-`+digits → DaysBack; `!`+digits → NewestLast; dashed-digit-shaped → Date; **all-digits → dir token** (request = SinceLastCall — capture N7d); `W`/anything else → Invalid (mirrors F's swallowed `F W`). Then optional dir token via `parse_span_token` (`None` → Invalid — captured `N R -1` → Argument error), then optional `Q`, then optional `NS` in help order; trailing junk → Invalid.

Menu-advert gate (four synchronised, compiler-forced edits — `menu_command.rs:1253-1293`, `:1303-1331`, `:1336-1364`): `advertised_token` arm `NewFilesScan(_) => Some("N")`; an `every_menu_command()` sample; a 4-space-indented `N [date]  Scan for new files since <date>` row in `Conf02/Menu5.txt` FILES section (:23-26); rewrite the N-is-Unknown pins — **three assertions across two test fns** (`menu_command.rs:764-765` and `:771`; the second fn also pins `MS 1`, which must be preserved).

### 3.2 Date handling (Clock port, pure functions in `new_files.rs`)

- `resolve_request(request, now, last_call: Option<SystemTime>) -> ScanKind` (+ label via `dir_row::format_dir_date`). Day arithmetic and formatting via the `time` crate in **UTC** (the `dir_row.rs:15` rendering precedent; recorded in COMMAND_PARITY). Cutoff = UTC midnight of the target day; filter `uploaded_at >= cutoff` — inclusive (`express.e:27976-27986`, SCAN sibling capture).
- Default/`S` day = `last_call` (capture-proven §1.1); `None` → today (TO-CONFIRM #12, a NextExpress choice). `DaysBack(x)` = today − x (capture: 07-03 − 30 → `06-03-26`). Two-digit-year pivot `yy > 77 → 19yy else 20yy` (`axconsts.e:41`, `MiscFuncs.e:434`); `year: None` → current year (TO-CONFIRM #6). Calendar validation via `time::Date::from_calendar_date` (strictness TO-CONFIRM #7).
- `parse_date_answer(answer)` for the prompt: empty → default; `mm-dd[-yy]` → Date; `-x` → DaysBack; `R` (optionally one following date token, ignored — N4b) → Reverse; anything else → `Error in date!` (`T`/`Y`/`S`/`!x` at the prompt provisional-error, TO-CONFIRM #2).
- Determinism: unit tests set `services.clock = Arc::new(ManualClock::set_to(…))` (precedent `menu_flow/tests.rs:739`); smokes use `TestRuntime::with_clock` (`tests/support/mod.rs:88-91` — its doc already names the N scan). App code never calls `SystemTime::now()` (architecture gate `rust/tests/architecture.rs:336`).

### 3.3 Port method (the July-review seam)

```rust
// domain/files/repository.rs — the reserved name (:10-12), now landing
/// Listing-visible files of `area` uploaded at or after `since` (inclusive),
/// same visibility filter and ordering contract as [`find_in_area`]
/// (`uploaded_at` ascending, insertion-order tiebreak).
/// # Errors
/// [`FileRepositoryError::Backend`] when the backing store fails.
fn list_new_since(&self, area: FileAreaRef, since: SystemTime)
    -> Result<Vec<File>, FileRepositoryError>;
```

Required, not defaulted. Day-boundary conversion lives caller-side; the port compares raw `SystemTime`. Implement in `InMemoryFileRepository::select` (`in_memory_file_repository.rs:36-45`, one filter line) **and in the test-local `FailingFileRepository`** (`file_list/tests.rs:1785`). Note: the door's genuine per-file filtering replaces internal N's dump-rest-of-DIR-after-first-match quirk (`express.e:27991-28013`) — unobservable under our sorted repository; equivalence row in COMMAND_PARITY.

### 3.4 Handler flows (`new_files.rs::handle_new_files(&mut self, session, arg)`, dispatched beside `FileList` at `menu_flow/mod.rs:788`)

**Prompt path** (bare `N`): (1) conference/areas/max as F does (`session.current_conference_number()`, `areas_in_conference`); (2) write the preamble `[reset, NEW_FILES_BANNER, blank]` **raw, not page-counted** — the door resets its counter at prompts (N2 pin); (3) default label = `format_dir_date(last_call.unwrap_or(now))`; (4) `prompt_line(session, &wire::date_prompt(&label), EmptyMeaning::Keep, AbortNotice::Silent)` — `Kept` → default, `Aborted` → flush-return, bad answer → `CRLF + ERROR_IN_DATE + b"\r\n\r\n\x1b[0m\r\n"` single-shot (N5); (5) `prompt_line(directories_prompt(max), Verbatim, Silent)` — empty → `b"\r\n\x1b[0m\r\n"` (N1a); `parse_span_token` `None` → F's `ERROR_IN_INPUT` envelope (TO-CONFIRM #3); out-of-range → F's `highest_dir_error` envelope (captured N8b); (6) only now take `flagged = session.flagged_files_mut()` (both prompts already stamped `record_input`; `ScanState` created after the prompts, so no borrow dance), `ScanState::new(false)`, emit the post-answer blank **through** `emit_scan_line` (counted — pins N2's 29-line page 1), then `run_span(…, &mode)`.

**Inline path** (`Scan(spec)`): resolve mode from spec + clock + `session.user().last_call()`; `span = spec.span.unwrap_or(FileSpan::Upload)`; `ScanState::new(spec.non_stop)`; feed `[reset, banner, blank]` **through** the pager via `begin_listing` (counted — N7c's 29-from-reset pin); `run_span`.

**Help**: write `wire::NEW_FILES_HELP_SCREEN` + flush. **Invalid**: F's argument-error envelope (reset line + `HELP_BANNER` + blanks) with `NEW_FILES_ARGUMENT_ERROR` (N7e).

**`H` under a date scan (interim, TO-CONFIRM #1 — must not panic):** ship `NewSince`/`NewestLast` + `Hold` as date-filtered/newest-filtered **held** rows, header built by the same dir→HOLD substitution F's engine uses (`wire::scanning_hold_header`, `wire.rs:398-401`) applied to the N header builders — flagged PLAUSIBLE in COMMAND_PARITY and listed as an open decision (§8). One probe re-pins or replaces it.

### 3.5 Conference/area iteration

Current conference only (`session.current_conference_number()`, the menu loop guarantees a joined conference — `mod.rs:196` comment). **No CF-flag involvement**: `ScanFlag::FileScan` (`domain/conference.rs:363-365`) gates only the future logon-scan slice; the `ScanFilter::MailScanFlagged` pattern (`scan_all_mail/core.rs:65-136`) remains the template for *that* slice, not this one. Access: NextExpress gates neither F nor N today; internal N gates `ACS_FILE_LISTINGS` (never the unused `ACS_NEW_FILES_SINCE`, `axcommon.e:12`) — consistent, recorded.

### 3.6 N wire consts ("Slice D9" section in `wire.rs`; every byte cited to `ae_tierd_newfiles.txt` section labels)

- `NEW_FILES_BANNER`: the branding transform on the captured N banner — centre label `AquaScan v1.0 by Aquarius/Outlaws ` (34 visible) → `NextScan ` (9 visible), dash run 15 → **40**, total **77 visible cols preserved**; right label `'n ?' for options ` is 18 visible cols, exactly `'f ?'`'s width, so this is the landed F banner (`wire.rs:60`) with the label letter swapped (rule: `designs/NEXTSCAN.md` §7). Test keeps the raw AquaScan original (Latin-1 `\xa9`-style bytes re-encoded to UTF-8 per the F precedent, `wire.rs:477-507`) and asserts width parity.
- `date_prompt(default: &str) -> Vec<u8>` — the full captured SGR sequence, ending `\x1b[0m ` (N1a; final space TO-CONFIRM #13).
- `ERROR_IN_DATE: &[u8] = b"Error in date!"` (N5; envelope written by the handler, mirroring F's junk-answer choreography).
- `scanning_new_header(n, label, found)` → `Scanning dir {n} for {label}... Ok!/Nothing found!` (N1b/N2/N3/N8).
- `scanning_newest_header(n, count, found)` → `Scanning dir {n} for the last {count} files... Ok!/Nothing found!` (N7n; edges TO-CONFIRM #8).
- `NEW_FILES_ARGUMENT_ERROR = b"Argument error! Type 'n ?' for help."` (N7e).
- `NEW_FILES_HELP_SCREEN: &str` — byte-exact from N6 with the branding swaps: Copyright banner → the landed NextScan `HELP_BANNER` (`wire.rs:80-81`, byte-reused), `Configure AquaScan` → `Configure NextScan`; every other line verbatim. `N W` itself is **not ported** (→ Invalid, the F W precedent) — needs Paul's sign-off (§8).
- Byte-reused unchanged: `directories_prompt`, `ERROR_IN_INPUT`, `highest_dir_error`, `HELP_BANNER`, `MORE_PROMPT` + the whole pager verb wire, `END_OF_FILE_LIST`, separators/frames/markers, `scanning_dir_header` (reverse arm serves N's R mode), `LISTING_EXIT_TAIL`, both overprint clears, `Quit` echo.
- `assemble_dir_lines(files, conference, flagged, quick)`: `quick` truncates each file to its first `dir_row_lines` row before framing (N7q: MYDEMO's continuation absent, `[ File #3 ]` follows directly). F passes `false`.

---

## 4. Simplifications folded in / deferred

**Folded in** (all structure-only, nil wire risk): the shared `parse_span_token` (§2.3); one parameterised `overprint_clear(width)` (69/79, `mod.rs:789-803`); `begin_listing` preamble dedupe; inner-walk-returns-`ScanFlow` + single `finish_listing` wrapper; delete the dead `.trim()` at the zippy resolver call (`mod.rs:712` — `prompt_line` already trims, `menu_flow/mod.rs:831`); move `ScanLine`/`ListedRow` from `wire.rs` into `scan.rs`; share the `9_999_999` size-bound literal between `frameable` (`wire.rs:260`) and `dir_row::size_column` (`dir_row.rs:60`) as one named const; hoist `CaptureTerminal`/`menu_session`/`test_user` into `test_support.rs`; fix the stranded advertised-menu doc fragment (`menu_command.rs:1009-1011`).

**Deferred** (recorded in SYSTEM.md): moving zippy to `file_list/zippy.rs` (cheap, but doubles same-slice test churn); the multi-conference section layer + pluggable pause style + `ScanFlow::SkipGroup` (SCAN/NSU + logon-confScan seam); crate-wide fake-terminal consolidation (8 variants — bigger blast radius than item 17); bundling `ScanState`+`FlaggedFiles` into a context struct (N's borrow dance disappears anyway since its state is created after the prompts); the per-path single-reset abort tails stay as named inline literals (**pinned asymmetry** — `mod.rs:27-30` doc; do not unify); lazy `SectionSource` (only if a future consumer's catalogue size demands it); `F <dir> Q` quick-scan for F (door supports it; landed F swallows `Q` as Invalid — follow-up item).

---

## 5. Ordered TDD step plan (failing-test-first; ~15–20 min steps)

**Phase A — extraction (no behaviour change; `tierd_file_list_smoke.rs` untouched and green after every step):**
1. Hoist `CaptureTerminal`/fixtures to `test_support.rs`; re-point both existing test modules. Gate: nextest green.
2. Create `scan.rs`; move engine impl block + free items **verbatim**; `mod scan;` in `file_list/mod.rs`. Gate: nextest green, `cargo build` warning-free.
3. Move the ~28 engine tests (content partition, §2.4) to `scan/tests.rs`, paths only. Gate: green; total test count unchanged (59 across the split).
4. Failing table-driven test for `parse_span_token` (A/a/U/H/h/digit/`2abc`/`-1`/junk); implement in `menu_command.rs`; swap the three call sites one at a time — the divergent-envelope tests are the non-regression proof.
5. Failing engine-API test: `run_span` with `ScanMode::Full{reverse}` (signature refactor; F's **two** call sites `mod.rs:169`/`:215` updated). Fold `begin_listing`, the walk/wrapper restructure, `overprint_clear(width)` one at a time, full suite green after each.
6. `make mutants-diff` (crate-relative diff per the memory note); document equivalent mutants (unreachable preamble-Quit arms, `row_contains_ci` totality branch). Update SYSTEM.md item 17 → landed (refresh its stale line refs: :103-116→:148-161, :799-818→:858-877, 862→921, 2,265→2,328). Commit phase A.

**Phase B — N behaviour (capture-pinned):**
7. Failing test: `list_new_since` on `InMemoryFileRepository` — inclusive boundary via the seeded 06-09/06-10/06-10 trio (cutoff 06-10 → 2 files; 06-11 → 0); ordering; **BADUPLD.LHA INCLUDED when in range** (it is Available with display check char `'F'`, `seed.rs:99`, and the door lists it — N2 File #19); listing-visibility contract via a constructed `HeldForReview` fixture. Implement in the adapter **and** `FailingFileRepository`.
8. Failing parser table for the full grammar (every N7 form, `N R -1`→Invalid, `N W`→Invalid, `N ?`, bare `N`, `N 7`, lowercase forms, `NS`-token non-binding); add the variant → fix `advertised_token` (compile error), `every_menu_command`, `Menu5.txt` row, rewrite the three N-placeholder assertions preserving the `MS 1` co-pin.
9. Failing wire tests: banner width math vs the kept AquaScan N original; `date_prompt` bytes; both headers; `NEW_FILES_HELP_SCREEN` literal + width; `ERROR_IN_DATE`.
10. Failing tests: pure date fns under `ManualClock` (2026-07-03 12:00 UTC — the capture day): pivot 77/78 both sides, `-30` month underflow → `06-03`, T→`07-03`, Y→`07-02`, `mm-dd` year default, junk, `R <date>`.
11. Failing tests: Help + Invalid envelopes (full byte streams from N6/N7e, rebranded).
12. Failing test: engine `NewSince` mode — N3 replay (`-30` → header, PROTRACK as `[ File #1 ]`, renumbered filtered set, End-of-File-List, post-End More?, Q tail).
13. Failing tests: prompt path — N1a (Enter/Enter abort), N1b (dir 2 Nothing found), N5, N8b, N4/N4b (R → Full-reverse), junk-at-dirs-prompt (PLAUSIBLE-flagged), H interim behaviour (§3.4), default-label test with ManualClock ≠ seeded `last_call`; the N2 page-1 pin (More? after exactly 29 counted lines from the post-answer blank; `?`-redraw window = those 29).
14. Failing tests: inline paths — N7a (Upload default), N7c (29-from-reset boundary; mid-list Q → `Quit\r\n` + tail), N7q, N7ns (zero `More?` substrings), N7n (`!2`; plus the saturating `!999` edge), N7t/y/s/d, N7r; F/R flag verb + repaint inside a `NewSince` scan (filtered numbering in the registry; `(conference, name)` keys).
15. New smoke `rust/tests/tierd_newfiles_smoke.rs`: in-process `TestRuntime` + demo catalogue + `with_clock(ManualClock 2026-07-03 12:00 UTC)`; **two-session shape** (session 1 logs off to stamp `last_call`, clock advances, session 2 drives N) pinning the default label; restated literals independent of prod consts; UTF-8 decode gate over the whole session; `write_key`/`read_idle` echo checks; `G Y` teardown.
16. Docs (§7), `make mutants-diff`, kill or document survivors; AGENTS.md step 6 type-at-a-real-terminal pass; like-for-like FS-UAE comparison (parent task #5).

---

## 6. Test / smoke / mutation plan (kill-shot map)

- **Boundary mutants** (`>=`→`>`): step 7's same-date pair. **Day-arithmetic**: exact-label asserts crossing a month boundary and the 77/78 pivot both sides (construct fixtures — the seed corpus has no cross-year data). **Default-source** (`last_call`→`now`): step 13's diverging-clock test + the two-session smoke. **Mode dispatch**: each `ScanKind` arm has header-literal + body tests; `found` has Ok!/Nothing-found asserts per mode. **Counter mutants**: two distinct page-1 pins (29-from-blank N2 vs 29-from-reset N7c) plus the untouched F pins. **Flag mutants** (`quick`, `non_stop`): N7q asserts the continuation *absent*; N7ns asserts zero `More?`. **Parser arms**: the exhaustive grammar table; `N 2`-is-a-dir kills date/dir classification swaps. **Envelopes**: full-stream equality on every error path. **Saturation**: the `!999` test kills a `saturating_sub`→`-` mutant.
- Phase-A moved lines re-mutate wholesale under `mutants-diff` — accepted cost; pre-existing equivalent mutants documented, not padded with fake tests. No concurrent cargo runs (memory note).
- The smoke restates every literal independently (the `tierd_file_list_smoke.rs` convention) and participates in the UTF-8 gate pattern.

---

## 7. Doc updates and parity/departure records

- **SYSTEM.md**: item 17 → landed (final shape: `file_list/scan.rs` sibling impl, `ScanMode` instead of caller-supplied rows + why, shared `parse_span_token` with per-caller envelopes, test-support hoist); refresh stale line refs; diagram gains `scan.rs`/`new_files.rs`; new deferred items: zippy move, SCAN/NSU + logon-confScan section seam, fake-terminal consolidation, `F <dir> Q`.
- **slices/cmds-files-list.md D9**: correct scope — single current-conference scan via the door's own Directories prompt; CF `FileScan` does **not** gate menu N (capture + `express.e:591-608`/`:28089`); full inline grammar (T/Y/S/`!x`/Q/NS) in scope; internal looping date prompt stays diff-record; mark the old multi-conference/Skip-Conf text superseded by `ae_tierd_newfiles.txt`.
- **COMMAND_PARITY.md** rows: N banner/help branding swaps (label widths, `Configure NextScan`, Copyright banner reuse); single-shot `Error in date!` vs the internal's looping length-only prompt (diff record); default date = previous-call day via `last_call` vs internal `newSinceDate` + `newSinceFlag` logoff bump (`express.e:27855`, `:27902`, `:8197` — diff record: NextExpress does not model `newSinceDate`); UTC day-boundary convention; uniform-29 paging vs the door's drifting pages 2+ (COSMETIC, F precedent); per-file filtering vs dump-rest-of-DIR (equivalence); ACS note (`ACS_FILE_LISTINGS`, unused `ACS_NEW_FILES_SINCE`); `N W` not ported; a PLAUSIBLE row per §1.2 item; `specs/core.allium:277-286` gets a clarifying note (the `file_scan` flag comment overstates its reach — logon scan only).
- **designs/**: this brief lands as `designs/2026-07-03-n-newfiles-scan-design.md`; `NEXTSCAN.md` §7 gains the N banner row; parent task #3 formally byte-pins every const against `ae_tierd_newfiles.txt` line numbers.
- **Auto-memory**: extend the Tier D notes with the N ground truth (single-conf, R-discards-date, counter-reset-at-prompt pager model, last-call default evidence).

---

## 8. Risks and open decisions for Paul

**Open decisions:**
1. **`H` under a date scan** (§3.4): ship the interim date-filtered-held behaviour with the substitution-derived header (PLAUSIBLE), or gate `H` off at N's prompt until the probe? The prompt *advertises* `(H)old`, so some defined behaviour must ship.
2. **`N W`**: advertised by the (rebranded) help screen but deliberately not ported (→ Argument error, the F W precedent) — a user-visible dead end needing explicit sign-off.
3. **Scope**: Q/`!x`/T/Y/S are captured and included, making D9 larger than the slice text envisioned. If it must shrink, the only safe cut is parsing those tokens to `Invalid` (captured envelope) and deferring — never a guessed partial implementation.
4. **Provisional answers** for TO-CONFIRM #2/#3/#5/#6/#7 — accept the stated defaults now (each quarantined behind its own const/test, one-line fix on re-probe), or hold phase B steps 13–14 for a follow-up probe session first?

**Risks:**
- **Byte-pin freeze during migration**: moved engine tests must keep entering through `handle_file_list` with assertions verbatim; rewriting them against the new API invalidates every `lines[..29]`/repaint pin. Treat any `tierd_file_list_smoke.rs` edit during phase A as a red flag.
- **Two page-counting models in one command** (prompt path resets at prompts; inline path counts the preamble) — easy to implement backwards; both boundaries carry dedicated pins (N2, N7c). Implemented caller-side (where `ScanState` is created) so the engine stays single-model; a future "unification" would silently break parity — the two pins are the guard.
- **`assemble_dir_lines` gains `quick`**: a defaulting mistake at an F call site would drop F continuations; F's continuation tests cover it only if run against the new signature.
- **`last_call` coupling**: if a future slice changes when `last_call` mutates, N's default silently becomes "today" — keep the two-session smoke.
- **Mutants-diff cost**: the moved engine (~500 lines) re-mutates wholesale; budget the run and document the known equivalent survivors up front.
- **Menu asset drift**: adding the `N` row to `Conf02/Menu5.txt` changes the `?` menu wire for every session; the like-for-like FS-UAE pass must account for the menu now differing from the shipped AquaScan board's.
- **Date flakiness**: every N test pins the clock (`ManualClock` unit-side, `.with_clock` smoke-side); the seeded corpus has no file after any plausible pinned "now" — add a fixture if future-dated handling needs an assertion.
