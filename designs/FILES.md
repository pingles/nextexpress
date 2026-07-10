# File Storage Design

How NextExpress persists file-area metadata and serves it under concurrent
load. Pairs with [`specs/files.allium`](../specs/files.allium), which
defines the behaviour this design supports.

## Scope

This document covers durable storage and search for the `File`, `Transfer`,
and `FlaggedFile` entities, file metadata (`FILE_ID.DIZ`, archive
comments, content kind, hashes), plus the access patterns the rules in
`files.allium` impose. It does **not** cover:

- Wire protocols for the actual byte transfer (Zmodem, Xmodem, etc.) —
  those live in the protocol library and stream blobs directly to/from
  disk, never through the metadata store.
- The concrete blob-store implementation. Mutable `(area, name, status)`
  must not be the durable blob identity: moves and status transitions should
  remain metadata operations. The publication/recovery contract is part of
  the accepted constraints below; its filesystem/S3 implementation stays an
  adapter concern.
- Anti-virus / archive validation hooks — configurable, out of domain.

## Sizing

The blob bytes ("a few hundred MB to a few GB") are not the constraint.
What matters for storage design is the **row count** behind those bytes:

| Avg file size | Files in 1 GB |
|---|---|
| 50 KB | ~20,000 |
| 500 KB | ~2,000 |
| 5 MB | ~200 |

Worst case is ~50K rows of `File`. `Transfer` grows with usage but is
append-mostly and rarely participates in search.

Concrete seed data still fits that shape:

| Corpus | Files | Bytes | Avg file size |
|---|---:|---:|---:|
| ASCII/ANSI art | ~6,000 | ~400 MB | ~67 KB |
| BBS archives | ~20,000 | ~1 GB | ~50 KB |
| Combined | ~26,000 | ~1.4 GB | ~54 KB |

The added constraint is **metadata extraction**, not file count. SQLite
stores file metadata, including the display description and `FILE_ID.DIZ`
content when present. It does not store cached raw text files or cached
ASCII/ANSI render payloads. Rendering a text/art file opens the original
blob from the blob store; at ~67 KB per art file this is acceptable and
lets the OS page cache do the hot-path work.

## Constraints and decisions

The July 2026 pre-transfer review settled the transaction and runtime
topology below. These choices are architectural constraints for D2s and the
transfer slices rather than implementation options for each adapter.

### Accepted constraints

- Every durable file has a stable `FileId`; identity-sensitive reads return a
  resolved value such as `LocatedFile { id, area, file }`. `FileAreaRef` is
  only an area address. A re-upload after removal creates a new `FileId` so
  historical transfers never appear to refer to replacement content.
- Transfer attempts and confirmed batches have stable begin tokens and
  ordinals. Begin and completion are idempotent, and the whole confirmed batch
  is resolved/reserved before bytes flow.
- The flag aggregate enforces the spec's 1000-entry cap. D-S2 removes matching
  flags from offline persistence and active sessions. Migration keeps the
  first 1000 entries in the existing aggregate's deterministic
  `(conference, name)` sort order and quarantines/reports any overflow instead
  of silently deleting it.
- Existing `Z` behaviour remains exact: case-insensitive substring matching
  over rendered DIR rows, emitted in area/catalogue order. DIZ search can only
  be a separately visible extension.
- Binary transfer uses a text-bypassing terminal mode and bounded buffers.
  Download content is range-readable; staged uploads are seekable and expose
  explicit commit/abort plus abandoned-transfer cleanup.
- An immutable opaque `blob_key` owns bytes. Area, name and status are metadata,
  so move/status rules do not rename the durable blob.
- Catalogue display order is `uploaded_at ASC` with an explicit per-area
  placement sequence as the stable timestamp tie-break, matching the landed
  repository contract. `MoveFile` can assign a collision-free target-area
  sequence without changing `FileId`; its exact tie placement is live-captured
  before that rule ships.
- `accounting_day` is fixed at `BeginDownload` from the board's daily-reset
  offset and is never recomputed at completion.

### Accepted user and architecture decisions

| Decision | Accepted design | First consumer |
|---|---|---|
| Metadata topology and configuration | Durable mode uses one SQLite metadata database/pool and one transaction-owning unit-of-work for users, memberships, flags, files and transfers. Incompatible split `user_storage`/`file_storage` configuration fails at startup. Ephemeral mode implements the same unit-of-work contract in memory. | D2s / D-T2 |
| Durable call identity | An opaque application-generated `CallId` is created after successful authentication and stored directly on `AuthenticatedCall`. The transfer ledger persists that value; node numbers are never durable call identity. | `AuthenticatedCall` / D-T1 |
| File names and duplicate resolution | Active duplicate names are allowed across areas. Legacy name-only lookup resolves in configured area order and then retains the resolved `FileId`; re-upload after removal always creates a new `FileId`. | D2s / D-T1 |
| Flag resolution, order, migration and concurrent saves | Preserve insertion order with an ordered sequence plus membership index and persist ordinals. Save add/remove deltas rather than replacing a user's set. Resolve to `FileId` at the transfer boundary. Migration keeps the first 1000 entries in the old aggregate's `(conference, name)` order and quarantines/reports overflow. Live capture still pins observable order. | D/DS / D-T2 |
| File-area identity and reconciliation | Configuration carries an opaque stable area key. Boot reconciliation matches that key, allows display number/name changes, tombstones removed areas and never repurposes their database IDs. | D2s |
| Extended search | V1 keeps legacy `Z` exact and omits DIZ/archive-comment search. Any later metadata search is a separately visible command or mode. | D2s |
| Async database boundary | Async application-facing facades submit synchronous repository work to bounded blocking workers. SQLite uses one serialized writer and a small bounded read pool; pure domain rules remain synchronous. Extraction/password work has separately bounded capacity. | D2s / D-T1 |
| Reservation source | Unfinished transfer rows are canonical reservations. Any cached user total is only a same-transaction rebuildable projection. | D-T2 / D-T3 |
| Commit durability | The unified durable metadata database uses WAL with `synchronous = FULL`. | D2s / D-T1 |
| Transfer retention | Retain the transfer ledger indefinitely in the first release. No automatic pruning lands without a separate archive/compaction design and a verifiable projection baseline. | D-T1 |

Representation choices for `FileId`, application-generated `TransferId` /
`TransferBatchId`, per-area catalogue tie-break sequence, folded filename,
range I/O, temporary publication and recovery are engineering consequences of
these constraints rather than optional behaviour.

The schema and ports below express these accepted choices.

### SQLite for durable file metadata

- **WAL mode** from day one. Readers don't block readers; readers don't
  block writers; one writer at a time.
- **`PRAGMA synchronous = FULL`** for the unified durable metadata database.
  Transfer attempts and accounting acknowledgements must survive power loss.
- **`busy_timeout` of a few seconds** so transient writer queueing
  surfaces as latency, not errors.
- **Prepared statements** for the hot queries.

All durable metadata shares this database. Narrow repositories may expose
separate domain-facing ports, but the unit-of-work owns cross-aggregate writes.

### `Transfer` is the accounting ledger

`Transfer` rows are the durable facts for upload/download accounting and
the U/D log. User, conference-membership, and file counters are cached
projections maintained for fast legacy-style displays and eligibility
checks; if a projection disagrees with completed transfers, the transfer
ledger wins and the projection can be rebuilt.

The unified metadata adapter treats transfer completion as an idempotent
writer transaction:

1. claim the unfinished transfer row (`finished_at IS NULL`);
2. write `finished_at`, `outcome`, `bytes_transferred`, and `cps`;
3. apply direction- and outcome-specific projections: completed downloads
   update file/download counters and charge only counters the spec defines as
   billable; completed uploads update upload counters and the pending file's
   state and size; failed uploads transition the pending metadata so staged
   content can be recovered or discarded;
4. commit the transfer and projection changes together.

Upload time bonus is a session effect, not a SQLite projection. Completion
records the computed `session_bonus_seconds` on the transfer and returns an
effect keyed by `TransferId`; the application applies that effect once to the
live session, which tracks already-applied transfer effects for the call. A
process loss also ends the in-memory call, so recovery never pretends the
session mutation was atomic with the metadata commit.

Merely pointing independent repository connections at the same file is not
enough. A metadata unit-of-work owns the
connection and transaction which executes transfer, file, user and membership
statements. Narrow repositories may remain as read/query facades over the same
database, but transfer completion must not call separately-transactional ports.

Retrying completion for the same `transfer_id` must not apply accounting a
second time. The unfinished-row claim is the idempotency guard.

Fix and denormalise accounting context onto the transfer row at
`BeginDownload`/`BeginUpload`:
`conference_number`, `area_id`, `area_number`, `accounting_day`,
`is_free_download`, and
`billable_bytes`. Audit queries and projection rebuilds should not change
meaning if a file later moves area or a conference's free-download setting
changes. Completion never recomputes these values.

For strict daily byte caps, download start reserves billable bytes with a
guarded writer transaction. Failed/cancelled transfers release the
reservation; completed transfers convert the reservation into
`daily_bytes_downloaded`. This prevents concurrent FTP transfers for the
same user from all passing the same stale pre-flight check.

The whole confirmed batch is resolved and reserved in one transaction. Each
attempt carries a stable application-generated begin token, batch identity and
ordinal, so retrying an ambiguous start returns the existing rows rather than
reserving twice. Unfinished attempts also carry enough progress/lease time for
the recovery policy to release abandoned reservations safely.

### Connection strategy

- Async application-facing facades submit synchronous SQLite operations to a
  bounded blocking executor; repository work never blocks a Tokio session
  task.
- One writer is serialized so a metadata unit-of-work owns one connection and
  transaction. A small bounded read pool serves snapshots; WAL lets those
  readers proceed while the writer commits.
- Never hold a connection, transaction, or synchronous mutex guard across an
  `.await`.
- The present synchronous `FileRepository` implementation remains usable
  behind the application facade. A synchronous method which merely locks a
  pooled connection on the Tokio task does not satisfy this boundary.
- Password hashing and extraction/scanning use separately bounded worker
  capacity so slow CPU/filesystem work cannot starve metadata requests.
- Apply connection-local pragmas (`foreign_keys`, `busy_timeout`, journal and
  synchronous policy) on every opened/pooled connection, not only the first.

### Content store and publication constraints

`FileContentStore` is not a whole-file `Vec<u8>` API and not merely a
forward-only stream. Downloads open a range-readable source so Zmodem can
honour ZRPOS/retry/resume without buffering the blob. Uploads open a bounded,
seekable staged sink with `commit` and `abort`; cancellation closes the sink
without publishing it.

Committed content receives an immutable opaque `blob_key` derived from stable
identity or content identity, never from mutable area/name/status. Publishing
a filesystem object and committing SQLite cannot be one atomic operation, so
the adapter uses an explicit recoverable sequence: write and fsync/close a
temporary object, publish atomically, commit its metadata reference, then
remove abandoned temporaries. Startup/reaper work reconciles temporary blobs,
final published blobs which have no committed metadata reference, unfinished
transfers, expired reservations and stale `in_playpen` rows. Orphan final
objects are deleted only after a grace period so recovery cannot race a live
publisher.

Progress/lease updates let recovery distinguish an active resumed transfer
from an abandoned one. The precise grace period is policy/configuration; the
existence of recovery is not optional.

### Metadata and DIZ extraction

Original file bytes remain in the blob store whether they are archives,
plain text, ANSI art, or binary files. SQLite stores metadata extracted
once during ingest/upload:

- `files.description` — short listing text used by directory/list views.
- `file_metadata.file_id_diz` — `FILE_ID.DIZ`, archive comment, or
  equivalent descriptive metadata.
- `file_metadata.content_kind` — `archive`, `text`, `ansi_art`, or
  `binary`.
- `file_metadata.blob_key`, hashes and format details needed to find and
  verify the original blob.
- lightweight text/art metadata such as encoding, line count, and ANSI
  detection.

`FILE_ID.DIZ` is treated as metadata because it describes the archive. Full
text/art file contents are not metadata and are not cached in SQLite.
Preview/render paths open the blob store object directly.

Per `specs/files.allium`, file visibility does not wait on DIZ extraction.
`CompleteUpload` moves the file to `held_for_review` or `available` based on
policy, and the `FileIdDizExtracted` rule can fire later against any of
`in_playpen`, `held_for_review`, or `available`. Updating a file's
description or DIZ metadata happens in a writer transaction that also
refreshes the FTS row.

### Extraction execution and safety budgets

Extraction is CPU-bound work over untrusted input. It must not block the
session loop, must not starve other users, and must not let a hostile
upload wedge the box. The cost varies sharply by container format:

| Format | Single-entry DIZ extraction cost |
|---|---|
| ZIP | Central directory at EOF → seek, decompress one entry. Bounded. |
| LHA / LZH / ARJ / ZOO / ARC | Sequential headers with size fields → walk and decompress one entry. Cheap. |
| TAR (plain) | Scan 512-byte headers, copy DIZ bytes. Uncompressed; fast. |
| TAR.GZ / TAR.BZ2 / TAR.XZ | Stream compression over the whole tar — must decompress from byte 0 until DIZ is found. Can stop early; worst case scans the lot. |
| LZX, DMS, ADF, other disk images | Treat as full-decompression cost. |

For the LHA/ZIP/ARJ files that dominate the seed corpus this is
milliseconds; a large `.tar.gz` with DIZ at the end can take seconds.
Hostile inputs (zip bombs, malformed headers that loop forever) need a
hard ceiling regardless of format.

Decisions:

- **Off the session thread.** Extraction runs inside `spawn_blocking` (or
  a dedicated rayon pool) so decompression does not stall the Tokio
  reactor serving other users. Cheap per-extraction overhead is fine at
  BBS cadence; the goal is isolation, not throughput.
- **Bounded extraction worker pool.** Cap concurrent extractions below CPU
  count, sized independently of the connection pool. One adversarial
  upload cannot saturate the host.
- **Hard per-job budgets**, enforced inside the worker:
  - wall-clock timeout (e.g. ~30 s archives, ~5 s text/art);
  - decompressed-bytes ceiling — abort on suspicious compression ratios
    rather than streaming gigabytes for one DIZ;
  - DIZ payload cap of a few KB; truncate rather than grow unbounded.
  A Tokio timeout around `spawn_blocking` does not stop the underlying worker;
  parsers must check cooperative byte/deadline budgets internally, or an
  uncooperative external tool must run in a killable subprocess.
- **Best-effort, not gating.** A timeout, abort, or absent DIZ does not
  fail the upload and does not block the status transition that
  `CompleteUpload` performs. The file keeps whatever status it landed in;
  `file_id_diz` stays NULL and `description_source` reflects whichever
  non-DIZ source supplied the description (or `none` if none did).
- **Same writer transaction on success.** When extraction does yield a
  DIZ, the update to `file_metadata.file_id_diz` (and the listing
  description, if it was empty) refreshes FTS atomically — readers never
  see a half-applied row. Apply it with a file status/version predicate so a
  late worker cannot resurrect metadata after deletion, replacement or a
  conflicting sysop edit.

This puts DIZ extraction in the same operational neighbourhood as the
spec's `BackgroundCheck` rule: async work that follows upload completion,
runs under bounded resources, and updates the file row through the same
writer connection as everything else.

### Search indexing candidate: FTS5 trigram

The shipped legacy-parity `Z` (zippy) command performs a
case-insensitive substring search across every rendered DIR row for each
file. That includes the filename row, size/date/check-character columns,
and continuation description rows. It emits the whole file block when any
row matches. A description-only query would therefore be a behaviour
change.

SQLite's FTS5 **`trigram` tokenizer** remains a useful candidate index,
but the indexed corpus must follow the accepted legacy corpus. One parity-preserving
shape is to index a normalized representation of the exact rendered search
text. Another is to use FTS to find candidates and verify each candidate
against the capture-pinned legacy row renderer before emitting it:

```sql
SELECT f.id
FROM files_fts
JOIN files AS f ON f.id = files_fts.rowid
JOIN file_areas AS a ON a.id = f.area_id
WHERE files_fts MATCH ?
  AND f.area_id IN (...)
  AND a.active = 1
  AND f.status IN (...); -- normal span: available/lcfiles; H: held_for_review
```

Status selection is rule input, not an FTS concern. Normal `Z` candidates are
limited to `{available, lcfiles}` and an `H` span to held files before exact
row verification. `in_playpen`, `quarantined`, `removed`, and inactive-area
rows never leak through the candidate index.

Substring "graph" decomposes into trigrams `gra`, `rap`, `aph`; FTS5
intersects the posting lists and verifies candidates. At ~26K rows, an
index over listing metadata remains comfortably inside SQLite's expected
scale.

Searching `FILE_ID.DIZ` / archive comments is a proposed NextExpress
extension, not part of the captured `Z` corpus. Do not silently mix it into
the parity path. Full text/art file search is likewise out of v1 unless a
separate user-visible feature and measurement justify it.

FTS is only a candidate accelerator, never the parity authority. Trigram
search cannot produce candidates for one- or two-byte queries, and MATCH
syntax must not reinterpret user punctuation as query operators. Fall back to
the exact renderer scan for short/unsupported literals; otherwise escape the
literal, fetch candidates, verify every candidate against the rendered rows,
and emit in requested area plus legacy catalogue order rather than FTS rank.

### `FlaggedFile` — per-session aggregate with landed persistence

- Lives on the session struct. The required cap is
  `max_flagged_files()` (1000); the current `FlaggedFiles` aggregate does
  not yet enforce it and must do so before D/DS.
- The landed implementation restores through `FlaggedStore` on logon and
  replaces the whole set on logoff. The accepted change retains restoration,
  persists add/remove commands when they occur, and leaves logoff responsible
  only for its parity banner plus any adapter flush.
- Two adapters wired by `config.user_storage` (the same switch that
  selects the user repository):
  - `InMemoryFlaggedStore` (default) — process-lifetime; a restart clears it.
  - `SqliteFlaggedStore` — durable; a `flagged_files (slot_number,
    conference, name)` table in the same `users.db` as the user store.
- Keying on disk is `(conference, name)` — and since the July 2026
  identity fix the domain `FlaggedKey` is exactly that pair too (the
  legacy `{confNum} {fileName}` format / `isInFlaggedList` identity,
  `express.e:12534`), so persistence is a lossless projection and a
  restored flag paints the `[X]` marker in listings.
- Replace the current `BTreeSet` with an insertion-ordered sequence plus a
  membership index and persist an ordinal. Live capture pins the observable
  batch order, but container iteration order never chooses it implicitly.
- A flag does not identify an area. Duplicate names remain legal across areas;
  `(conference, name)` resolves in configured area order and then retains the
  stable `FileId`. D10 confirmed that the pager's `F` name entry, like manual
  `A`, accepts one unchecked trimmed/upper-cased whole line (including unknown
  or space-containing names), so either source may remain unresolved until
  D/DS preflight.
- Once resolved, `FileId` is authoritative and is never silently retargeted to
  a same-named replacement. Preflight reloads that row and rechecks current
  status, active area and caller access. A same-conference move keeps the
  binding. A cross-conference move or rename updates the legacy key in the
  metadata unit-of-work; on a destination-key collision the earlier ordinal
  wins and the dropped binding is reported. A transition out of
  `{available, lcfiles}`, deletion, or area tombstone purges the durable flag.
  Active-session reconciliation uses item 32's typed control plane; the first
  such mutation cannot ship without it.
- D-S2 deletion must fan out to both persisted sets belonging to offline
  users and in-memory sets in active sessions. A SQL foreign-key cascade
  alone cannot implement that while flags retain the legacy key, and the
  current `FlaggedStore` exposes no store-wide purge operation.
- Replace-all logoff saving is not safe under concurrent sessions for one
  account: the last logoff wins, and a session holding a stale set can reinsert
  a flag after D-S2 purges it. Persist add/remove commands when they occur;
  never replace the whole durable set at logoff.

## Schema sketch

This is the cumulative final-state schema, not one D2s migration. D2s
establishes the unified database, stable file/area identity and file metadata;
D-T1 adds durable transfer attempts; D-T2, D-T3 and D-T4a add the user and
membership accounting fields their rules consume. Each slice lands only its
portion through the shared versioned migration mechanism.

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = FULL;
PRAGMA foreign_keys = ON;

-- Conference configuration remains file-backed. stable_key is the opaque
-- identity carried by configuration; numbers and names may change.
CREATE TABLE file_areas (
    id                  INTEGER PRIMARY KEY,
    stable_key          TEXT    NOT NULL UNIQUE,
    conference_number   INTEGER NOT NULL,
    area_number         INTEGER NOT NULL,
    name                TEXT    NOT NULL,
    free_downloads      INTEGER NOT NULL DEFAULT 0,
    active              INTEGER NOT NULL DEFAULT 1,
    CHECK (free_downloads IN (0, 1)),
    CHECK (active IN (0, 1))
);
CREATE UNIQUE INDEX idx_file_areas_active_number
    ON file_areas (conference_number, area_number)
    WHERE active = 1;

CREATE TABLE files (
    id              INTEGER PRIMARY KEY,
    area_id         INTEGER NOT NULL REFERENCES file_areas(id),
    name            TEXT    NOT NULL,
    name_folded     TEXT    NOT NULL,         -- same ASCII fold as FlaggedKey
    size_bytes      INTEGER NOT NULL CHECK (size_bytes >= 0),
    status          TEXT    NOT NULL,         -- FileStatus enum
    check_char      INTEGER,                  -- nullable ASCII P/F/N/D byte
    description     TEXT    NOT NULL DEFAULT '',
    description_source TEXT NOT NULL DEFAULT 'none',
    description_private_to_sysop INTEGER NOT NULL DEFAULT 0,
    uploaded_by_slot INTEGER REFERENCES users(slot_number),
    uploaded_at     INTEGER NOT NULL,         -- epoch seconds
    catalogue_sequence INTEGER NOT NULL,      -- per-area placement order
    last_downloaded_at INTEGER,
    download_count  INTEGER NOT NULL DEFAULT 0,
    row_version     INTEGER NOT NULL DEFAULT 0,
    CHECK (status IN (
        'in_playpen', 'held_for_review', 'available', 'lcfiles',
        'quarantined', 'removed'
    )),
    CHECK (description_source IN (
        'none', 'uploader_supplied', 'sysop_supplied',
        'internal_file_id_diz'
    )),
    CHECK (check_char IS NULL OR check_char IN (80, 70, 78, 68)),
    CHECK (description_private_to_sysop IN (0, 1)),
    CHECK (download_count >= 0),
    UNIQUE (area_id, catalogue_sequence)
);

-- Removed rows retain their identity for audit. Re-upload creates a new row
-- and therefore a new FileId.
CREATE UNIQUE INDEX idx_files_area_name_active
    ON files (area_id, name_folded)
    WHERE status <> 'removed';
CREATE INDEX idx_files_area_uploaded_at
    ON files (area_id, uploaded_at ASC, catalogue_sequence ASC);
CREATE INDEX idx_files_status_pending    ON files (status)
    WHERE status IN ('held_for_review', 'quarantined', 'in_playpen');

CREATE TABLE file_metadata (
    file_id          INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
    blob_key         TEXT    NOT NULL,
    sha256           TEXT    NOT NULL,
    content_kind     TEXT    NOT NULL, -- 'archive' | 'text' | 'ansi_art' | 'binary'
    archive_format   TEXT,
    file_id_diz      TEXT,
    file_id_diz_path TEXT,
    file_id_diz_crc32 TEXT,
    text_encoding    TEXT,
    ansi_detected    INTEGER NOT NULL DEFAULT 0,
    line_count       INTEGER,
    metadata_extracted_at INTEGER
);

-- Illustrative contentless trigram index. legacy_search_text is a
-- normalized representation of the exact capture-pinned DIR rows (or a
-- candidate corpus followed by exact row verification). extended_metadata
-- is queried only if the separate DIZ/archive-comment extension is chosen.
-- The FTS table stores index postings, not raw file text or render payloads.
CREATE VIRTUAL TABLE files_fts USING fts5 (
    legacy_search_text,
    extended_metadata,
    content='',
    contentless_delete=1,
    tokenize='trigram'
);

-- Trigger/application-maintained refresh is chosen after the exact legacy
-- search-text representation and optional extension mode are settled.

-- Final-state command/delta-based flag shape. Migration assigns deterministic
-- ordinals to the current three-column rows. resolved_file_id stays nullable
-- because legacy `A` can flag an unchecked name; D/DS resolves it before
-- transfer.
CREATE TABLE flagged_files (
    user_slot          INTEGER NOT NULL REFERENCES users(slot_number) ON DELETE CASCADE,
    conference_number INTEGER NOT NULL,
    name               TEXT    NOT NULL,
    name_folded        TEXT    NOT NULL,
    resolved_file_id   INTEGER REFERENCES files(id),
    ordinal            INTEGER NOT NULL CHECK (ordinal >= 0),
    flagged_at         INTEGER NOT NULL,
    PRIMARY KEY (user_slot, conference_number, name_folded),
    UNIQUE (user_slot, ordinal)
);
CREATE INDEX idx_flagged_resolved_file ON flagged_files (resolved_file_id);

-- Migration preserves, but does not activate, entries beyond the 1000 cap and
-- reports affected users to the operator.
CREATE TABLE flagged_files_overflow (
    user_slot          INTEGER NOT NULL REFERENCES users(slot_number) ON DELETE CASCADE,
    conference_number INTEGER NOT NULL,
    name               TEXT    NOT NULL,
    name_folded        TEXT    NOT NULL,
    original_ordinal   INTEGER NOT NULL CHECK (original_ordinal >= 1000),
    quarantined_at     INTEGER NOT NULL,
    PRIMARY KEY (user_slot, conference_number, name_folded)
);

-- The cumulative unified-metadata schema also contains these projections.
-- Exact DDL lands with its consuming D slice through the migration module;
-- these are required final fields, not all D2s fields:
-- users: bytes_downloaded_total, bytes_uploaded_total,
--        daily_bytes_downloaded, daily_byte_limit, accounting_day
-- conference_memberships: bytes_uploaded, bytes_downloaded,
--        files_uploaded, files_downloaded, ratio_mode, ratio_value
-- Reservation is canonical on unfinished transfer rows. A user-level reserved
-- total, if retained for display/performance, is a rebuildable same-transaction
-- projection and must be reconciled with designs/USERS.md.

CREATE TABLE transfers (
    id              TEXT    PRIMARY KEY,      -- application-generated TransferId
    batch_id        TEXT    NOT NULL,         -- application-generated TransferBatchId
    batch_ordinal   INTEGER NOT NULL CHECK (batch_ordinal >= 0),
    call_id         TEXT    NOT NULL,         -- opaque CallId created at authentication
    user_slot       INTEGER NOT NULL REFERENCES users(slot_number),
    file_id         INTEGER NOT NULL REFERENCES files(id),
    conference_number INTEGER NOT NULL,       -- denormalised accounting context
    area_id         INTEGER NOT NULL REFERENCES file_areas(id),
    area_number     INTEGER NOT NULL,          -- historical display/accounting context
    direction       TEXT    NOT NULL,         -- 'upload' | 'download'
    accounting_day  INTEGER NOT NULL,         -- epoch start of board reset bucket
    started_at      INTEGER NOT NULL,
    last_progress_at INTEGER NOT NULL,
    finished_at     INTEGER,
    bytes_transferred INTEGER NOT NULL DEFAULT 0 CHECK (bytes_transferred >= 0),
    billable_bytes  INTEGER NOT NULL DEFAULT 0 CHECK (billable_bytes >= 0),
    reserved_billable_bytes INTEGER NOT NULL DEFAULT 0 CHECK (reserved_billable_bytes >= 0),
    session_bonus_seconds INTEGER NOT NULL DEFAULT 0 CHECK (session_bonus_seconds >= 0),
    cps             INTEGER NOT NULL DEFAULT 0 CHECK (cps >= 0),
    outcome         TEXT,                     -- TransferOutcome enum, null until finished
    is_free_download INTEGER NOT NULL DEFAULT 0 CHECK (is_free_download IN (0, 1)),
    CHECK (direction IN ('upload', 'download')),
    CHECK (outcome IS NULL OR outcome IN (
        'completed', 'aborted_by_user', 'carrier_lost',
        'failed_integrity_check', 'skipped_duplicate', 'skipped_other',
        'failed_no_space', 'failed_ratio'
    )),
    CHECK ((finished_at IS NULL) = (outcome IS NULL)),
    CHECK (finished_at IS NULL OR finished_at >= started_at),
    UNIQUE (batch_id, batch_ordinal)
);

CREATE INDEX idx_transfers_user_started ON transfers (user_slot, started_at DESC);
CREATE INDEX idx_transfers_file_started ON transfers (file_id, started_at DESC);
CREATE INDEX idx_transfers_user_day ON transfers (user_slot, accounting_day, direction, outcome);
CREATE INDEX idx_transfers_call ON transfers (call_id, started_at, batch_ordinal);
```

## Access patterns and how they map

| Spec rule | Query | Index used |
|---|---|---|
| `FlagFile` | retain the legacy `(conference, name)` key; resolve duplicates in configured area order and retain the resulting `FileId` | `idx_files_area_name_active` per candidate area + configured area order |
| `displayFileList` | list by area in `uploaded_at` order with placement sequence as the stable tie-break; a "since X" scan applies the same order after its cutoff | `idx_files_area_uploaded_at` |
| Legacy `zippy` substring search | search/verify the exact rendered DIR-row corpus in selected areas | trigram candidate index plus exact verification, or a precomputed exact corpus |
| Optional extended metadata search | query DIZ/archive comments only through the explicitly chosen extension | trigram FTS, if the extension is accepted |
| ASCII/ANSI preview | lookup `blob_key`, open original blob | `file_metadata` PK + blob store |
| `CheckDownloadEligibility` | sum `size_bytes` for ≤1000 ids | PK lookups; small N |
| Strict daily byte cap | reserve billable bytes for the idempotent confirmed batch | metadata unit-of-work + unfinished-transfer/user projection query |
| `BeginDownload` / `BeginUpload` | idempotently insert the ordered transfer batch after resolving stable file IDs | transfer/batch IDs + writer transaction |
| `CompleteDownload` | finish transfer once + additive projection updates | transfer PK + user/file PK |
| `CompleteUpload` | finish transfer once + additive projection updates | transfer PK + user/file PK |
| Sysop "held files" | list by status | `idx_files_status_pending` |
| `MoveFile`, `DeleteFile`, `AdmitHeldFile` | update by identity; move allocates a collision-free target-area sequence | PK + area/sequence unique key |
| User U/D log | history by user | `idx_transfers_user_started` |
| Rebuild charged download totals | `SUM(bytes_transferred)` for completed, non-free downloads | `idx_transfers_user_day` / user scan |
| Rebuild upload totals | `SUM(bytes_transferred)` for completed uploads | `idx_transfers_user_day` / user scan |

## Concurrency

For 32+ concurrent BBS users:

- **Reads parallelize.** WAL gives every reader a consistent snapshot at
  the moment its transaction begins; concurrent SELECTs do not block one
  another and do not block the writer.
- **Writes are bounded and serialized in the unified metadata database.** At BBS
  cadence (transfer completions, status changes, occasional admin) this
  is far below SQLite's write throughput.
- **Same-user sessions compose through deltas.** Interactive BBS sessions
  and future FTP sessions may share a `user_slot`. Transfer completion never
  writes a stale `User` snapshot back to SQLite; it applies additive
  deltas derived from one completed transfer.
- **Transfer completion is idempotent.** The ledger writer only completes
  an unfinished transfer row. Replayed completion calls return the
  already-recorded outcome without charging a second time. Projection
  updates share the same metadata transaction.
- **Transfer start is idempotent.** A retry with the same application begin
  token returns the existing ordered batch and does not reserve again.
- **Abandoned work is recoverable.** Progress leases let a bounded reaper
  release reservations and reconcile temporary blobs/`in_playpen` rows after
  a crash without racing an active resumed transfer.
- **Strict quota checks reserve capacity.** If a daily byte limit must be
  enforced across concurrent sessions, `BeginDownload` reserves billable
  bytes in the writer transaction. Without that reservation, limits are
  advisory under parallel FTP transfers.
- **`FlaggedFile` writes are command-based.** A flag/unflag updates the
  session aggregate and applies the corresponding durable add/remove command.
  Logoff no longer replaces the whole set. This makes same-account sessions
  compose and prevents a stale logoff from undoing D-S2 purge.
- **Extraction runs on a separate worker pool.** DIZ extraction and any
  future archive/AV checks live on a bounded `spawn_blocking` (or rayon)
  pool sized below CPU count and independent of the connection pool. One
  pathological `.tar.gz` cannot stall the Tokio reactor or starve other
  users; per-job timeouts and decompressed-byte ceilings cap the worst
  case.
- **Long-running read transactions are forbidden.** A reader that holds
  a transaction open across user input pins WAL growth. Read, materialise
  the result, release.
- **Blob content is streamed with back-pressure.** A transfer must not load
  an entire blob into a session-owned `Vec<u8>`. Reads/writes are chunked,
  cancellation-safe, and isolated from the terminal's text rendering path.

## Adapter layout

```
domain/files/
  file.rs              — File entity, FileStatus state machine, invariants
  identity.rs          — required FileId/LocatedFile identity values
  flagged.rs           — FlaggedFiles/FlaggedKey (per-session set)
  flagged_store.rs     — FlaggedStore port + FlaggedStoreError (slice D5-persist)
  transfer.rs          — Transfer entity, outcome accounting
  repository.rs        — query-oriented FileRepository
app/transfer/
  metadata.rs          — async metadata unit-of-work port + command/results
adapters/
  metadata/
    sqlite.rs          — shared pool, serialized writer + unit-of-work
    in_memory.rs       — ephemeral implementation of the same boundary
  in_memory_flagged_store.rs  — landed pre-unification adapter; migrated into metadata owner
  sqlite_flagged_store.rs     — landed pre-unification facade; borrows shared owner after D2s
adapters/files/
  sqlite_files.rs    — query facade borrowing the shared metadata read pool
  fs_blob_store.rs   — blob open/stage/publish/delete on disk
  file_metadata.rs   — DIZ/comment extraction + content metadata
```

(`legacy_dir.rs`, the read-only ingest of AmiExpress DIR text files,
was dropped 2026-06-10: no legacy on-disk data compatibility — see
`slices/cmds-files-list.md`'s parity-target section.)

The `FileRepository` port stays narrow: methods named after the rules
that need them (`find_in_area`, `list_new_since`, the eventual exact
zippy-search query, `find_metadata`, `list_by_status`), not generic CRUD.
Adapter is free to add indexes for the queries the domain actually uses.

### Port prep decisions (July 2026, review item 18 — landed)

Settled ahead of the `N` slice so D2s inherits the right contract:

- **Area addressing.** The port takes `FileAreaRef { conference, area }`
  (`domain/files/area.rs`), the file-world `MessageBaseRef` analogue
  and the prefix of the catalogue lookup key `(conference, area, name)`
  (the active-row folded-name index above). No more raw `(u32, u32)` pairs, so
  conference/area transpositions are unrepresentable. This is an area
  address, **not** stable file identity: it does not survive a move or
  rename and cannot be the transfer ledger's `file_id` by itself.
- **Fallibility + error shape.** The four read methods return
  `Result<_, FileRepositoryError>` now, while the only adapter is
  infallible — the signature break is paid once, before N/V/VS add
  call sites. `FileRepositoryError` is one opaque
  `Backend { source: Box<dyn Error + Send + Sync> }` variant per the
  port-error convention (SYSTEM.md item 2); rich diagnostics stay
  adapter-private.
- **Error policy at the listing.** A backend failure logs and renders
  exactly what an empty catalogue renders (the legacy wire for an
  unreadable DIR file is the empty listing); pinned by the
  `failing_repository_renders_like_an_empty_catalogue` equivalence
  test. The policy lives in one place
  (`file_list/mod.rs::empty_on_error` + the three read helpers).
- **Operations land with their consuming slices** (the schema-growth rule):
  the query-only `list_new_since` method lands with the `N` scan. Transfer,
  file, user and membership writes for D-T2/D-T4a execute through the selected
  metadata unit-of-work, not separately transactional repository methods.
  `File::transition_to` and missing `FileStatus` variants land with the first
  rule that moves a file through them.
- **Content vs metadata.** File *content* is not this port's concern. A
  separate streaming `FileContentStore` port (open/stage/publish/delete of
  blob bytes — `fs_blob_store.rs` above) arrives with the first transfer
  slice (D-T1). Its application-facing operations are async, range-readable
  for downloads and bounded/seekable for staged uploads.
- **Concurrency.** No per-area lock registry until a writer slice
  demonstrates the need: reads are `&self` snapshots. The previous plan
  to copy the synchronous `users.db` `Mutex<Connection>` pattern into
  D2s is rejected: database work crosses the selected async-facade/bounded-
  worker boundary before file traffic increases it.

## Things to nail down

- ~~**Round-tripping the legacy DIR text format.**~~ Settled 2026-06-10:
  neither read nor write — no legacy data compatibility. Listings are
  generated at runtime from repository data; the DIR row *format*
  survives only as part of the rendered wire output (the AquaScan-style
  listing re-renders the same fields — see
  `comparison/evidence-tierD/live-observations.md`).
- **Extended search scope.** V1 omits DIZ/archive-comment and full-text/art
  search. Revisit only as a separately visible feature after measurement;
  legacy `Z` remains the exact captured DIR-row corpus.
- **`Transfer` retention.** The first release performs no automatic deletion.
  Revisit only with an archival/compaction design and a verifiable projection
  baseline.
- **Flag lifecycle beyond persistence.** Cross-session persistence is
  landed. D-S2 still needs store-wide offline purge plus active-session
  purge, after duplicate-name resolution and stable file identity are
  settled.
- **Area reconciliation.** Boot upserts by the configuration's stable area
  key, updates its display conference/area number and name, and tombstones
  missing keys. An area ID is never repurposed; transfers retain denormalised
  conference/area numbers for audit.
- **Extraction retry policy.** When DIZ extraction times out or aborts on
  budget exhaustion, do we leave the file with `file_id_diz = NULL`
  forever, expose a sysop-triggered re-extract, or schedule a single
  bounded retry? "Never retry" is the simplest default and matches
  best-effort semantics; revisit if operational logs show otherwise.
- **Worker pool sharing.** DIZ extraction and the spec's `BackgroundCheck`
  (AV / archive validation) share one bounded extraction/validation pool with
  per-job budgets. It is isolated from SQLite and password-hashing capacity so
  untrusted archives cannot starve authentication or metadata work.

## Future optimisations (not for v1)

Add only when measurement says so:

- **In-memory snapshot** of file metadata behind `ArcSwap` for lock-free
  reads. Worth it if a flamegraph shows SQL parsing or thread-pool
  dispatch dominating a tight search loop. At expected QPS (32 users
  × ~1 query/s) this is unlikely.
- **Per-area sub-snapshots** for cheaper rebuilds, if the snapshot
  approach is adopted.
- **External search index** (`tantivy`) — only if we want ranking,
  fuzzy matching, or phrase queries beyond what FTS5 trigram offers.
  The legacy `Z` semantics don't need this.
