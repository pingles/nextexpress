# Tier D (browse) — File commands without transfer

The file subsystem is the BBS's second pillar after messaging. This
file slices the *browsing* commands so users get visible value from
file areas long before the Zmodem transfer adapter lands.
Transfer-side slices live in
[cmds-files-transfer.md](cmds-files-transfer.md); sysop-only file
admin in [cmds-files-sysop.md](cmds-files-sysop.md).

See [SLICES.md](../SLICES.md) for the schema-growth principle and
asset inventory.

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
  - On-disk loader reads `Conf<n>/Dir<m>` (legacy layout) and
    surfaces filename, size, description, status.
- **Out of Scope**
  - BCD packing (storage-side concern noted in `core.allium:Bytes`).
  - Sysop-uploaded files (in `cmds-files-sysop.md`).
- **Why first**: every downstream slice depends on these entities.

## Slice D2 — `F` (file listings, read-only)

- **In Scope**
  - Parser: `MenuCommand::FileList(NumberArg)` mirroring
    `internalCommandF` (`amiexpress/express.e:24877`).
  - Wire output mirrors the legacy paged listing
    (`displayFileList(params)`), column widths and the
    `[32m`-prefixed column headers.
- **Out of Scope**
  - Flagged-files integration (slice D5).
  - Pause / break-out keystrokes — those follow the `non-stop` flag
    legacy handling, which is its own slice once `H`'s `NS` token
    paths are unified.

## Slice D3 — `FR` (reverse listing)

- **In Scope**
  - Same code path as D2 but passes `reverse = true` through to the
    adapter — exactly the legacy split at
    `amiexpress/express.e:24887`.
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
  - Flag is set when a user types `F <filename>` *after* the listing
    has been displayed — exactly the legacy single-listing flow.
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
  - Walks every area in the current conference (or every area
    flagged for file-scan if `F` was set in `CF`), listing files
    whose `uploaded_at` is newer than the user's last call.
- **Out of Scope**
  - File-scan-all-conferences (legacy doesn't have a `NS`
    convention; defer if asked).

## Slice D-wire — Tier D (browse) wire-and-smoke

- **In Scope**
  - Seed a small fixture file area on disk; smoke test runs `F`,
    `FR`, `Z foo`, `A`, `FS`, `N` against the running binary and
    asserts the legacy wire bytes.
- **Out of Scope**
  - Asserting against a real lha — deflate / extraction is a
    transfer-side concern.
