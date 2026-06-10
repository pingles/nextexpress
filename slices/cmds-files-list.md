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

## Slice D1 — `Bytes` value type + `FileArea` + `File` entities

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

## Slice D2 — `F` (file listings, read-only)

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

## Slice D3 — `FR` (reverse listing)

- **In Scope**
  - Same code path as D2 with `reverse = true`: banner label
    `'fr ?' for options` (dash run flexes), header
    `Reverse-scanning dir N... Ok!`, files emitted newest-first.
  - Captured quirk to honour: bare `FR` skips the directories
    prompt and starts at the highest dir, descending.
    (The internal split at `amiexpress/express.e:24887` is the
    shadowed stock path — diff record only.)
- **Out of Scope**
  - Sorting on fields other than upload-date.

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

- **In Scope**
  - `files.allium:FlagFile`, `UnflagFile`, with the per-session
    flagged list bounded by `max_flagged_files()` (legacy
    `MAX_FLAGGED_FILES = 1000`).
  - `FlaggedFilesAreDownloadable` invariant.
  - Flag is set from the listing pager, exactly as captured: `F` (or
    `R`) at the `More?` prompt erases the prompt line and opens the
    line-read `File name(s) to flag: `; after the filename it returns
    **silently** to `More?` — no confirmation text
    (`ae_tierd_aquascan3.txt` S4). `C` clears.
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
  - Seed a small fixture file area; smoke test runs `F`, `FR`,
    `Z foo`, `A`, `FS`, `N` against the running binary and asserts
    the wire bytes — NextScan-branded AquaScan bytes for `F`/`FR`/`N`
    (cross-checked against `comparison/evidence-tierD/fixtures/` +
    the capture transcripts), internal-command bytes for the
    unshadowed `Z`/`A`/`FS`.
- **Out of Scope**
  - Asserting against a real lha — deflate / extraction is a
    transfer-side concern.
