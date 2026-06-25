# Tier D (browse) — File commands without transfer

The file subsystem is the BBS's second pillar after messaging. This
file slices the *browsing* commands so users get visible value from
file areas long before the Zmodem transfer adapter lands.
Transfer-side slices live in
[cmds-files-transfer.md](cmds-files-transfer.md); sysop-only file
admin in [cmds-files-sysop.md](cmds-files-sysop.md).

See [SLICES.md](../SLICES.md) for the schema-growth principle and
asset inventory.

## Parity target: the AquaScan door, with NextScan branding

The reference board **as shipped does not serve `internalCommandF`**
for `F` / `FR` / `N`: the stock deployment installs AquaScan v1.0 door
icons in `BBS:Commands/BBSCmd/`, and `processCommand`
(`amiexpress/express.e:28229-28256`) dispatches BBSCmd icons *before*
internal commands. The user-visible reference experience for these
tokens is AquaScan's, and that is what NextExpress implements
(decision 2026-06-10). Live byte-level ground truth, including the
stock-vs-AquaScan diff table and capture hazards, is in
[`comparison/evidence-tierD/live-observations.md`](../comparison/evidence-tierD/live-observations.md);
the cleanest transcript is `comparison/transcripts/ae_tierd_aquascan3.txt`.

**NextScan branding (deliberate departure).** Generated text must not
reuse the AquaScan name or credit. Exactly three captured strings carry
branding and get NextScan-branded replacements with the frame width
preserved by flexing the dash runs: the listing banner
(`--[ AquaScan v1.0 by Aquarius/Outlaws ]---…[ 'f ?' for options ]--`),
the help banner (incl. `Copyright © 1994 Aquarius`), and the help line
`F W - Configure AquaScan`. Every other byte matches the captures.

**No legacy data compatibility.** NextExpress does not read, ingest or
round-trip legacy `Conf<n>/DIR<m>` files (decision 2026-06-10). The
listing is generated at runtime from repository data — an in-memory
seeded fake for out-of-the-box boots (mirroring `app/seed.rs`), SQLite
per [`designs/FILES.md`](../designs/FILES.md) for real deployments.

Legacy refs to `internalCommandF` (`express.e:24877`) and
`displayFileList` (`:27626`) below identify the *shadowed internal*
path — kept for the stock difference record, not as the wire target.
`Z`, `A`, `FS` and friends have **no** door icons on the stock board,
so their slices still target the internal commands.

## Slice D1 — `Bytes` value type + `FileArea` + `File` entities — **Done**

Landed 2026-06-10/11 together with D2 (commits `6c15e03`, `cf4a595`).
`FileStatus` shipped with three variants — `available`, `lcfiles`
(the spec's listing-visible set, `files.allium:52-53`) and
`held_for_review` (what `F H` shows) — the remaining variants and the
transition table arrive with their first writers (schema-growth).
`File` carries the six browse fields plus `check_char: Option<u8>`
(the upload-writer's col-13 status byte, raw row data orthogonal to
`FileStatus`; now also recorded as `File.check` in the spec). The
conference/area association lives in the repository keying — no
browse rule reads `file.area` directly. The seed corpus mirrors
`comparison/evidence-tierD/fixtures/` byte-for-byte into the landing
conference so dev-boot listings are directly comparable to the live
captures.

- **In Scope**
  - **Expands** the existing `core.allium:Bytes` value type. `Bytes`
    already exists (`rust/src/domain/bytes.rs`, introduced in Slice 48
    for `MailAttachment.file_size`) but currently only has `new` /
    `count`; this slice adds the ordering, addition and saturating
    subtraction the file-area arithmetic needs.
  - `core.allium:FileArea`, `files.allium:File` (neither entity
    exists in the Rust tree yet) with the spec's
    `status` lifecycle (`available`, `in_playpen`, `held_for_review`,
    `lcfiles`, `quarantined`, `removed`).
  - Repository adapters surface filename, size, upload date,
    description (first line + continuations), uploader and status:
    the in-memory seeded fake (selected when no real file data is
    configured, so a dev boot always lists something) and the SQLite
    metadata store per `designs/FILES.md`. The exact port signatures
    and which adapter lands in which sub-step follow the D1+D2 design
    pass.
- **Out of Scope**
  - ~~On-disk loader for the legacy `Conf<n>/Dir<m>` layout~~ —
    **superseded 2026-06-10**: no legacy data compatibility (see the
    parity-target section above).
  - BCD packing (storage-side concern noted in `core.allium:Bytes`).
  - Sysop-uploaded files (in `cmds-files-sysop.md`).
- **Why first**: every downstream slice depends on these entities.

## Slice D2 — `F` (file listings, read-only) — **Done**

Landed 2026-06-10/11 (commits `4725b61`, `ee3521e`): the NextScan
lister reproduces the captured AquaScan UX end-to-end —
`rust/src/app/menu_flow/file_list/` (handler + `dir_row` + `wire` +
the 29-line `ScanPager`), `TerminalEcho::Silent` through the telnet
adapters, six-scenario telnet smoke
(`rust/tests/tierd_file_list_smoke.rs`). Scope notes vs. the original
plan, per the design pass (`designs/NEXTSCAN.md`) and the recapture
session: the `More?` verbs `C`/`F`/`R`/`?` shipped IN D2
(wire-identical; flag entries read-and-discarded until D5 wires
`FlaggedFile`; `?` emits the captured in-pager pause help + a page
redraw), the `NS` token shipped IN D2, lone `n` is the captured
buffered `N`/`ns` prefix (erased by the next verb — not a stop key),
and junk arguments take the captured help-banner
`Argument error! Type 'f ?' for help.` path. Mutation-tested to zero
missed across the unit's modules.

> **Superseded in part by slices D2u/D2b/D2f (2026-06-12..14).** The
> `TerminalEcho::Silent` adapter read and the read-and-discarded flag
> entries described above were interim D2 choices. D2u re-encoded the
> wire to UTF-8 (`&str` art/© constants; AGENTS.md "Wire encoding").
> D2b replaced the Silent line reads with true single-key hotkeys
> (`Terminal::read_key`) — echo on keypress, no extra Enter — and
> retired `TerminalEcho::Silent`. D2f makes `F`/`R` flag listed files
> into a session-scoped `FlaggedFiles` set, rendered as an on-row
> `[X]` marker and repainted in place; persistence and the door's
> downstream flag surfaces remain slice D5 (below). See
> `designs/2026-06-12-utf8-hotkeys-flagmark-design.md`.

- **In Scope**
  - Parser: `MenuCommand::FileList(…)` mirroring **AquaScan's token
    grammar** (from its captured `F ?` help): `F [R] dir [Q] [NS]`
    with dir = `U`pload / `A`ll / number / `H`old.
  - Wire output reproduces the captured AquaScan listing with
    NextScan branding: banner line, `Scanning dir N from top... Ok!`
    / `Nothing found!` / `Scanning HOLD dir from top...` headers,
    per-file frames (separator-art pair carrying the file date,
    `[ File #N ]` header row, colour-coded fields: cyan name to col
    13, blue status char, green size, yellow date, plain description
    with col-33 continuations), `[ End of File List ]` footer.
  - The `More?` hotkey pager: `Y`/unknown = continue, `Q` echoes
    `Quit` and exits, `ns` → `Non-stop scrolling! Are you sure
    (Y/n)? ` confirm, `?` re-shows help; post-end-of-list `n` is
    rejected (backspaced).
  - Bare `F`: the door's own
    `Directories: (1-N), (A)ll, (U)pload, (H)old, (Enter)=None ? `
    line prompt — Enter aborts silently, bad input → `Error in
    input!` and exit to menu.
- **Out of Scope**
  - `C`/`F`/`R` flag options at the `More?` prompt — flagging is
    D5/D6; the design pass settles the in-between behaviour for
    those keys.
  - The `Q`uick-scan token (first-description-line-only mode) — not
    yet captured live; capture before slicing.
  - `F W` (door self-configuration) — not ported; NextExpress config
    is TOML.
  - Flagged-files integration (slice D5).

## Slice D2s — SQLite file metadata store

- **In Scope**
  - `adapters/files/sqlite_files.rs` per
    [`designs/FILES.md`](../designs/FILES.md) (WAL,
    `synchronous = NORMAL`, prepared statements), serving the same
    three-method `FileRepository` port; a near-copy of
    `sqlite_user_repository.rs`'s shape.
  - The `file_storage` config key (reserved by D2's deferral): set,
    it opens/creates the SQLite store and the in-memory demo
    catalogue is **not** seeded — demo records never enter real
    data; unset, the seeded in-memory repository serves as today.
  - `Result` plumbing through the port (the first fallible adapter).
- **Why deferred from D2**: nothing can write real rows until the
  upload/maintenance slices land, so a SQLite adapter would have
  served only the dummy seed — a schema-growth violation. Scheduled
  no later than the first file-writer slice; pull forward if a
  hand-loaded real deployment is wanted sooner.

## Slice D2b — true single-key pager hotkeys

- **In Scope**
  - `Terminal::read_key` through both telnet adapters,
    `ColourTerminal` and every test fake; the NextScan `More?` /
    ns-confirm prompts switch from Silent line reads to single-key
    reads, implementing the captured `n`-buffering at the byte level.
- **Why deferred from D2**: with Silent line reads the server-emitted
  bytes are capture-identical, but the user must press Enter after
  each pager key — a documented COSMETIC/ergonomics divergence.
  Scheduled immediately behind the D1+D2 unit (user decision
  2026-06-10): the AquaScan feel is hotkey-driven.

## Slice D3 — `FR` (reverse listing) — **Done**

Landed 2026-06-16. The `FR` token reuses the D2 lister with one
`reverse: bool` threaded parser → handler → wire. Parity authority and
the bare-`FR` reconciliation are settled in
[`designs/2026-06-16-fr-reverse-listing-design.md`](../designs/2026-06-16-fr-reverse-listing-design.md):
AquaScan board-as-shipped owns the wire bytes and bare-`FR` control
flow; `express.e` (`displayFileList :27626`, `getDirSpan :26857`) fills
the gaps where the captures are silent.

- **In Scope (shipped)**
  - Same code path as D2 with `reverse = true`: banner label
    `'fr ?' for options` (dash run flexed 40→39 to hold 77 cols),
    header `Reverse-scanning dir N... Ok!` (no "from top"), files
    emitted newest-first (the area's rows reversed).
  - Grammar: `FR` is the concatenated reverse token
    (`express.e:28310` dispatches the whole code). `FR <n>`/`A`/`U`/`H`
    /`NS` mirror `F`. Bare `FR`, like bare `F`, opens the
    `Directories:` prompt (under the reverse `'fr ?'` banner) and then
    reverse-walks whichever span the caller picks — following
    `express.e:27645-27648` (`getDirSpan('')`) over the AquaScan
    capture (S11), which skips the prompt for `FR`. (Amended 2026-06-18,
    reversing the initial "bare `FR` skips the prompt" decision, per the
    "use the original code" rule.) The "descending" walk applies to
    multi-dir spans (`FR A` walks highest→lowest, `express.e:27654`
    reverse loop).
  - `F R` with a space is **not** an original reverse form (the
    original dispatch matches the whole `FR` token); it stays the
    `F`-with-junk `Argument error!` path.
- **Out of Scope**
  - Sorting on fields other than upload-date.
  - `F R` (space) modifier — an AquaScan-help-only grammar, uncaptured
    being exercised; deferred.
  - `FR ?` distinct help bytes (reuses the `F ?` help for now).

## Slice D4 — `Z` (zippy text search) — **Done**

Landed 2026-06-19. `Z` is the genuine internal command (`internalCommandZ`,
`amiexpress/express.e:26123`) — **not** AquaScan-shadowed (it is absent
from the door icon set), so unlike `F`/`FR`/`N` its parity target is the
internal command, captured live in
[`comparison/transcripts/ae_tierd_zippy.txt`](../comparison/transcripts/ae_tierd_zippy.txt)
(Z1–Z7) and [`ae_tierd_zippy2.txt`](../comparison/transcripts/ae_tierd_zippy2.txt)
(ZU/ZH/ZOOR). The wire is plain text — raw DIR rows via
`file_list::dir_row::dir_row_lines`, **no** NextScan frames or colour —
deliberately distinct from the colourful `F` door.

- **In Scope (shipped)**
  - Parser `MenuCommand::ZippySearch(ZippyArg)`: exact-token dispatch
    (`StrCmp(cmdcode,'Z')`, `:28388`), so `ZOOM` stays separate; the
    search string is the first parameter token (`item(0)`, `:26146`),
    bare `Z` is the prompt form (`:26150`).
  - Bare `Z` → the `Enter string to search for: ` prompt (plain, no ANSI);
    an empty answer returns (`:26155-26156`).
  - The internal `getDirSpan('')` `Directories: …=none? ` prompt
    (`:26864`) — **distinct** from AquaScan's `directories_prompt`
    (lowercase `=none?`, space after `?`, closing reset with no trailing
    space). Honoured answers: a directory **number** (single dir), `U`
    (upload = highest dir, rendered by number), `A` (all areas), `H`
    (hold), blank = `(Enter)=none` abort, and the out-of-range
    `No such directory.` error (`:26905`, distinct from AquaScan's
    highest-dir error).
  - Substring search via the legacy `UpperStr` + `InStr` over each
    rendered DIR row (filename row included, `:27595-27598`): any line of
    a file's block that contains the upper-cased query dumps the whole
    block (continuations included). Case-insensitive.
  - 11 handler unit tests (exact wire) + 2 binary-reachable telnet smokes
    (`tierd_file_list_smoke.rs`).
- **Scope note — what the live capture corrected.** The original plan
  ("search the current area only; the `A`/`1-3` area-spec is D7") was
  written before the reference was driven. The capture showed `Z`
  **always** opens the interactive `getDirSpan('')` directory prompt —
  there is no session "current area". D4 therefore ports the full
  interactive prompt (number/`U`/`A`/`H`/none/out-of-range). The *inline*
  `item(1)` area-spec **argument** (`Z <q> <span>`, the
  `getDirSpan(item(1))` path that skips the prompt) shipped right after,
  in **D7 below** — surfaced by user feedback that `Z ART 1` should scan
  immediately like the reference rather than re-prompt.
- **Out of Scope (deferred)**
  - Pagination of a large match set (legacy `flagPause`,
    `:27582`/`:27613`) — uncaptured (no search filled a screen); the
    seeded corpus never paginates. Capture before porting.
  - The `L` (reload) `getDirSpan` answer (`:26891-26894`) — an uncaptured
    edge; currently falls into `No such directory.`.
  - Wildcard / regex syntax.

## Slice D5 — `FlagFile` / `UnflagFile` rules

- **Already landed (slice D2f):** the per-session flagged set
  (`FlaggedFiles`/`FlaggedKey`), `F`/`R` flagging from the `More?`
  pager (erase prompt, line-read `File name(s) to flag: `, silent
  return — `ae_tierd_aquascan3.txt` S4), the on-row `[X]` marker, and
  the in-place repaint. D5 builds the rule layer and the downstream
  surfaces on top.
- **Landed (slice Ga):** the clean-logoff `checkFlagged` confirm.
  Plain `G` with a non-empty session flag set runs
  `confirm_leave_flagged()` — the `You have flagged files still not
  downloaded.` / `Do you leave without them? (y/N)?` prompt
  (`express.e:12667`/`:2129`, single-key `yesNo`, default N), returning
  to the menu on `N`; `G Y` (new force form), a `Y` answer, or an empty
  flag set log off. `MenuCommand::Logoff` now carries `auto`. Live
  reference captured to `comparison/transcripts/ae_tierd_g_confirm.txt`
  (flag a file via the AquaScan `F` verb → plain `G`, both branches);
  byte-pinned in `menu_flow/tests.rs` + an e2e wire smoke in
  `tierd_file_list_smoke.rs`.
- **Done (slice D5-banner)** — every `G` logoff now emits
  `saveFlagged()`'s visible `** AutoSaving File Flags **` banner +
  `<BEL>` (`AUTOSAVING_FILE_FLAGS`, `menu_flow/mod.rs`) before the
  goodbye tail, unconditionally — the banner precedes saveFlagged's own
  flag-count gate (`express.e:2803`), so it shows even with nothing
  flagged; only the Stay branch (plain `G` + flagged + `N`) skips it.
  Byte-pinned to `ae_tierd_g_confirm.txt:177` (flagged) and
  `ae_tierd_g_empty.txt` (empty) (`express.e:25064` → `:2803`); 4 unit
  tests in `menu_flow/tests.rs` + the e2e wire smoke in
  `tierd_file_list_smoke.rs`. **Deferred to D5-persist:** the per-slot
  `Partdownload/flagged` file write + `saveHistory()`, the cross-session
  restore, and the logon `** Flagged File(s) Exist **` banner.
- **In Scope (D5-persist, after the banner)**
  - `files.allium:FlagFile`, `UnflagFile`, with the per-session
    flagged list bounded by `max_flagged_files()` (legacy
    `MAX_FLAGGED_FILES = 1000`).
  - `FlaggedFilesAreDownloadable` invariant.
  - The remaining downstream flag surface the captures/E source show:
    the logon `** Flagged File(s) Exist **` + BEL banner
    (`amiexpress/express.e:2791-2794`, captured at transcripts line
    77), shown when a restored flag set is non-empty. (The clean-logoff
    `checkFlagged` "You have flagged files still not downloaded."
    warning landed in slice Ga and the `** AutoSaving File Flags **`
    logoff banner in slice D5-banner; D5-persist adds the per-slot file
    write that makes the flag set survive a disconnect, plus the logon
    banner that announces a restored set.)
  - A fresh capture session for AquaScan's own un-exercised in-door
    flag verbs (`A` alter-flags, `D` quit-and-download) before
    porting them (D6a/D6b own `A`).
- **Out of Scope**
  - Persisting the flagged list across sessions (open question in
    `files.allium`).

## Slice D6a — `A` (list flagged set, read-only)

- **In Scope**
  - `MenuCommand::AlterFlags` no-arg form: prints the current
    per-session flagged list using the legacy wire text
    (`alterFlags` listing rows at `amiexpress/express.e:24604`).
  - Pure read of the structure D5 already added; no mutation paths.
- **Why split**: users flag files in `F`/`Z` and then want to see
  what they've collected before downloading. The listing alone
  closes that loop and ships ahead of the edit grammar.

## Slice D6b — `A` (edit file flags, add / remove)

- **In Scope**
  - Interactive add / remove sub-prompt over the flagged list
    (`+filename`, `-filename`, list-by-area), with legacy wire text.
- **Out of Scope**
  - "Flag from outside the current conference" — the legacy permits
    cross-conference flagging in some configurations; deferred.

## Slice D7 — `Z` inline area-spec — **Done**

Landed 2026-06-19, immediately after D4, triggered by user feedback: on
the reference, `Z <term> <dir>` scans **immediately** (no `Directories:`
prompt), but D4 ignored the inline directory token and re-prompted — a
visible divergence. Captured live in
[`comparison/transcripts/ae_tierd_zippy3.txt`](../comparison/transcripts/ae_tierd_zippy3.txt)
(`Z ART 1`/`A`/`U`/`H`/`9`/`xyz`).

- **In Scope (shipped)**
  - Parser `ZippyArg::QueryInDir { query, span }`: `Z <token> <span>`
    captures the `item(1)` directory span; tokens past `item(1)` are
    dropped (`parseParams` reads only items 0 and 1).
  - The handler resolves the inline span with the **same** `getDirSpan`
    logic the interactive prompt answer uses (`resolve_zippy_span`:
    number / `A` / `U` / `H` / out-of-range), but **without** the
    `Directories:` prompt (`getDirSpan(item(1))` ELSE branch,
    `amiexpress/express.e:26162-26163`, `:26875-26877`). Two blanks
    (26137 + 26172) precede the first header; an out-of-range/junk span
    takes `No such directory.` immediately.
  - 6 handler unit tests + 1 binary-reachable telnet smoke; verified by
    hand against both the live server and the FS-UAE reference.
- **Out of Scope**
  - Numeric **ranges** (`Z <q> 1-3`) — the legacy `getDirSpan` takes a
    single `Val`, not a range; the captured forms are number/`A`/`U`/`H`.
  - The first-char-only `A`/`U`/`H` match (a token like `Apple` → all
    dirs on the legacy) — D7 matches the whole token, an uncaptured edge.

## Slice D8 — `FS` (file status / free space)

- **In Scope**
  - Reads the disk-level free space for the current conference's
    files directory and emits the legacy `fileStatus(0)` wire text
    (`amiexpress/express.e:24872`).
- **Out of Scope**
  - Per-area drive quotas — not modelled in the spec.

## Slice D9 — `N` (new files scan, file semantic)

- **In Scope**
  - Parser: `MenuCommand::NewFilesScan(params)` — replaces the
    placeholder no-op slot left by [cmds-mail-finish.md](cmds-mail-finish.md)'s
    B2 slice.
  - UX per the captured AquaScan flow: the
    `Date: (MM-DD-YY), (-X) Days, (R)everse, (Enter)…` prompt,
    `Error in date!` on bad input, then date-filtered scan headers
    `Scanning dir N for <mm-dd-yy>... Ok! / Nothing found!`
    (captured via the door's `SCAN`/`NSU` siblings, which share the
    engine — `ae_tierd_aquascan3.txt` S12, `ae_tierd_aquascan.txt`
    P2/P4).
  - Walks every area in the current conference (or every area
    flagged for file-scan if `F` was set in `CF`), listing files
    whose `uploaded_at` is newer than the requested date.
  - The internal `N`'s looping
    `Date as (mm-dd-yy) to search from (Enter)=: ` prompt is the
    shadowed stock path — diff record only (and a harness hazard:
    it consumes every line until a valid date).
- **Out of Scope**
  - File-scan-all-conferences (legacy doesn't have a `NS`
    convention; defer if asked).

## Slice D-wire — Tier D (browse) wire-and-smoke

- **In Scope**
  - Smoke coverage of the remaining tier commands as they land:
    `FR`, `Z foo`, `A`, `FS`, `N` against the running binary —
    NextScan-branded AquaScan bytes for `FR`/`N`, internal-command
    bytes for the unshadowed `Z`/`A`/`FS`. (`F`'s six-scenario smoke
    shipped with D2 — `rust/tests/tierd_file_list_smoke.rs`, driving
    the seeded demo corpus that mirrors
    `comparison/evidence-tierD/fixtures/`.)
- **Out of Scope**
  - Asserting against a real lha — deflate / extraction is a
    transfer-side concern.
