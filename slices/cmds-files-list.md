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
    /`NS` mirror `F`. Bare `FR` skips the `Directories:` prompt and
    reverse-scans the **upload/highest dir only** (maps to
    `FileSpan::Upload`, `ae_tierd_aquascan3.txt` S11) — a deliberate
    asymmetry with bare `F` (which prompts). The "descending" walk
    applies to multi-dir spans (`FR A` walks highest→lowest,
    `express.e:27654` reverse loop), not to bare `FR`.
  - `F R` with a space is **not** an original reverse form (the
    original dispatch matches the whole `FR` token); it stays the
    `F`-with-junk `Argument error!` path.
- **Out of Scope**
  - Sorting on fields other than upload-date.
  - `F R` (space) modifier — an AquaScan-help-only grammar, uncaptured
    being exercised; deferred.
  - `FR ?` distinct help bytes (reuses the `F ?` help for now).

## Slice D4 — `Z` (zippy text search, single-area first)

- **In Scope**
  - Parser: `MenuCommand::ZippySearch(query, area_spec)`.
  - Substring search of description text, **scoped to the current
    conference's current area only** in this slice.
  - Interactive prompt when query is missing
    (`amiexpress/express.e:26150-26156`).
- **Out of Scope**
  - Multi-area scans (the `A` / `1-3` area-spec — that's slice D7).
  - Wildcard / regex syntax.

## Slice D5 — `FlagFile` / `UnflagFile` rules

- **Already landed (slice D2f):** the per-session flagged set
  (`FlaggedFiles`/`FlaggedKey`), `F`/`R` flagging from the `More?`
  pager (erase prompt, line-read `File name(s) to flag: `, silent
  return — `ae_tierd_aquascan3.txt` S4), the on-row `[X]` marker, and
  the in-place repaint. D5 builds the rule layer and the downstream
  surfaces on top.
- **In Scope**
  - `files.allium:FlagFile`, `UnflagFile`, with the per-session
    flagged list bounded by `max_flagged_files()` (legacy
    `MAX_FLAGGED_FILES = 1000`).
  - `FlaggedFilesAreDownloadable` invariant.
  - The downstream flag surfaces the captures/E source show: the
    logon `** Flagged File(s) Exist **` + BEL banner
    (`amiexpress/express.e:2791-2794`, captured at transcripts line
    77), the clean-logoff `checkFlagged` "You have flagged files
    still not downloaded." warning (`express.e:12667-12673`), and the
    `** AutoSaving File Flags **` logoff banner (`express.e:2803`).
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

## Slice D7 — `Z` multi-area scan

- **In Scope**
  - Extends D4 to honour the area-spec parameter (`Z <q> A` for all
    accessible areas, `Z <q> 1-3` for a range).
  - `getDirSpan` parity (`amiexpress/express.e:26162`).

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
