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
- Blob storage layout on disk (one file per `File` entity, path derived
  from `(area, name)`; status-specific subdirs for playpen / held /
  quarantined). Treated as a separate adapter concern.
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

## Decisions

### SQLite as the single source of truth

- **WAL mode** from day one. Readers don't block readers; readers don't
  block writers; one writer at a time.
- **`PRAGMA synchronous = NORMAL`** — durability/throughput sweet spot
  in WAL mode. Use `FULL` only for the rare commits where it matters
  (currently none for files).
- **`busy_timeout` of a few seconds** so transient writer queueing
  surfaces as latency, not errors.
- **Prepared statements** for the hot queries.

### `Transfer` is the accounting ledger

`Transfer` rows are the durable facts for upload/download accounting and
the U/D log. User, conference-membership, and file counters are cached
projections maintained for fast legacy-style displays and eligibility
checks; if a projection disagrees with completed transfers, the transfer
ledger wins and the projection can be rebuilt.

The storage adapter therefore treats transfer completion as an idempotent
writer transaction:

1. claim the unfinished transfer row (`finished_at IS NULL`);
2. write `finished_at`, `outcome`, `bytes_transferred`, and `cps`;
3. for billable completed transfers, apply additive deltas to projections
   (`SET bytes_downloaded_total = bytes_downloaded_total + ?`, etc.);
4. commit the transfer and projection changes together.

Retrying completion for the same `transfer_id` must not apply accounting a
second time. The unfinished-row claim is the idempotency guard.

Denormalise accounting context onto the transfer row at start/completion:
`conference_id`, `area_id`, `accounting_day`, `is_free_download`, and
`billable_bytes`. Audit queries and projection rebuilds should not change
meaning if a file later moves area or a conference's free-download setting
changes.

For strict daily byte caps, download start reserves billable bytes with a
guarded writer transaction. Failed/cancelled transfers release the
reservation; completed transfers convert the reservation into
`daily_bytes_downloaded`. This prevents concurrent FTP transfers for the
same user from all passing the same stale pre-flight check.

### Connection strategy

- An async-friendly pool such as `tokio-rusqlite` (worker thread per
  connection) or `deadpool-sqlite`.
- Sized to roughly the CPU core count for read connections, plus one
  dedicated writer connection. With WAL, readers proceed in parallel.
- One connection per task; never share a connection across awaits.

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
- **Best-effort, not gating.** A timeout, abort, or absent DIZ does not
  fail the upload and does not block the status transition that
  `CompleteUpload` performs. The file keeps whatever status it landed in;
  `file_id_diz` stays NULL and `description_source` reflects whichever
  non-DIZ source supplied the description (or `none` if none did).
- **Same writer transaction on success.** When extraction does yield a
  DIZ, the update to `file_metadata.file_id_diz` (and the listing
  description, if it was empty) refreshes FTS atomically — readers never
  see a half-applied row.

This puts DIZ extraction in the same operational neighbourhood as the
spec's `BackgroundCheck` rule: async work that follows upload completion,
runs under bounded resources, and updates the file row through the same
writer connection as everything else.

### FTS5 with the trigram tokenizer for search

The legacy `Z` (zippy) command is substring search across file
descriptions in one or more areas. NextExpress extends the search corpus
to include `FILE_ID.DIZ` / archive-comment metadata. SQLite's FTS5 with
the **`trigram` tokenizer** gives us substring `MATCH` queries directly:

```sql
SELECT f.id
FROM files_fts
JOIN files AS f ON f.id = files_fts.rowid
WHERE files_fts MATCH ?
  AND f.area_id IN (...);
```

Substring "graph" decomposes into trigrams `gra`, `rap`, `aph`; FTS5
intersects the posting lists and verifies candidates. Index size scales
with description plus DIZ metadata, not with the 1.4 GB of original blobs.
At ~26K rows, DIZ/comment search remains comfortably inside SQLite's
expected scale. Full text/art file search is out of v1 unless measurement
says the extra content index is worth it.

### `FlaggedFile` — per-session set, optionally persisted (slice D5-persist)

- Lives on the session struct, capped at `max_flagged_files()` (1000).
- On logoff, the set is saved via the `FlaggedStore` port
  (`domain/files/flagged_store.rs`); on logon, it is restored.
- Two adapters wired by `config.user_storage` (the same switch that
  selects the user repository):
  - `InMemoryFlaggedStore` (default) — process-lifetime; a restart clears it.
  - `SqliteFlaggedStore` — durable; a `flagged_files (slot_number,
    conference, name)` table in the same `users.db` as the user store.
- Keying on disk is `(conference, name)`; `area` is dropped on save and
  restored as `0` (matches the legacy `{confNum} {fileName}` format;
  `area` is a NextExpress session-local concern the `F`/`R` pager uses).
- Cascade on file delete (per `DeleteFile` rule) is a fan-out across
  active sessions, not a SQL `ON DELETE`.

## Schema sketch

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;

CREATE TABLE files (
    id              INTEGER PRIMARY KEY,
    area_id         INTEGER NOT NULL REFERENCES file_areas(id),
    name            TEXT    NOT NULL,
    size_bytes      INTEGER NOT NULL CHECK (size_bytes >= 0),
    status          TEXT    NOT NULL,         -- FileStatus enum
    description     TEXT    NOT NULL DEFAULT '',
    description_source TEXT NOT NULL DEFAULT 'none',
    uploaded_by     INTEGER REFERENCES users(id),
    uploaded_at     INTEGER NOT NULL,         -- epoch seconds
    last_downloaded_at INTEGER,
    download_count  INTEGER NOT NULL DEFAULT 0,
    UNIQUE (area_id, name)
);

CREATE INDEX idx_files_area_uploaded_at  ON files (area_id, uploaded_at DESC);
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

-- Contentless trigram FTS for zippy / DIZ metadata search. The FTS table
-- stores index postings, not raw file text or cached render payloads.
CREATE VIRTUAL TABLE files_fts USING fts5 (
    description,
    file_id_diz,
    content='',
    contentless_delete=1,
    tokenize='trigram'
);

-- Keep FTS in sync with file descriptions and DIZ metadata. Implementations
-- may use DELETE+INSERT or INSERT OR REPLACE depending on the SQLite version.
CREATE TRIGGER files_ai AFTER INSERT ON files BEGIN
    INSERT INTO files_fts(rowid, description, file_id_diz)
    VALUES (new.id, new.description, '');
END;
CREATE TRIGGER files_au_desc AFTER UPDATE OF description ON files BEGIN
    DELETE FROM files_fts WHERE rowid = old.id;
    INSERT INTO files_fts(rowid, description, file_id_diz)
    VALUES (
        new.id,
        new.description,
        COALESCE((SELECT file_id_diz FROM file_metadata WHERE file_id = new.id), '')
    );
END;
CREATE TRIGGER files_ad AFTER DELETE ON files BEGIN
    DELETE FROM files_fts WHERE rowid = old.id;
END;
CREATE TRIGGER file_metadata_ai AFTER INSERT ON file_metadata BEGIN
    DELETE FROM files_fts WHERE rowid = new.file_id;
    INSERT INTO files_fts(rowid, description, file_id_diz)
    VALUES (
        new.file_id,
        COALESCE((SELECT description FROM files WHERE id = new.file_id), ''),
        COALESCE(new.file_id_diz, '')
    );
END;
CREATE TRIGGER file_metadata_au_diz AFTER UPDATE OF file_id_diz ON file_metadata BEGIN
    DELETE FROM files_fts WHERE rowid = new.file_id;
    INSERT INTO files_fts(rowid, description, file_id_diz)
    VALUES (
        new.file_id,
        COALESCE((SELECT description FROM files WHERE id = new.file_id), ''),
        COALESCE(new.file_id_diz, '')
    );
END;

CREATE TABLE transfers (
    id              INTEGER PRIMARY KEY,
    session_id      INTEGER NOT NULL,
    user_id         INTEGER NOT NULL REFERENCES users(id),
    file_id         INTEGER NOT NULL REFERENCES files(id),
    conference_id   INTEGER NOT NULL REFERENCES conferences(id),
    area_id         INTEGER NOT NULL REFERENCES file_areas(id),
    direction       TEXT    NOT NULL,         -- 'upload' | 'download'
    accounting_day  INTEGER NOT NULL,         -- daily-reset bucket
    started_at      INTEGER NOT NULL,
    finished_at     INTEGER,
    bytes_transferred INTEGER NOT NULL DEFAULT 0,
    billable_bytes  INTEGER NOT NULL DEFAULT 0,
    reserved_billable_bytes INTEGER NOT NULL DEFAULT 0,
    cps             INTEGER NOT NULL DEFAULT 0,
    outcome         TEXT,                     -- TransferOutcome enum, null until finished
    is_free_download INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_transfers_user_started ON transfers (user_id, started_at DESC);
CREATE INDEX idx_transfers_file_started ON transfers (file_id, started_at DESC);
CREATE INDEX idx_transfers_user_day ON transfers (user_id, accounting_day, direction, outcome);
```

## Access patterns and how they map

| Spec rule | Query | Index used |
|---|---|---|
| `FlagFile` | lookup by `(area_id, name)` | `UNIQUE (area_id, name)` |
| `displayFileList` | list by area, often "since X" | `idx_files_area_uploaded_at` |
| `zippy` substring search | `files_fts MATCH ?` over description + DIZ metadata | trigram FTS |
| ASCII/ANSI preview | lookup `blob_key`, open original blob | `file_metadata` PK + blob store |
| `CheckDownloadEligibility` | sum `size_bytes` for ≤1000 ids | PK lookups; small N |
| Strict daily byte cap | reserve billable bytes before transfer | writer transaction + user PK |
| `BeginDownload` / `BeginUpload` | insert `transfers`, lookup `files` row | PK + writer connection |
| `CompleteDownload` | finish transfer once + additive projection updates | transfer PK + user/file PK |
| `CompleteUpload` | finish transfer once + additive projection updates | transfer PK + user/file PK |
| Sysop "held files" | list by status | `idx_files_status_pending` |
| `MoveFile`, `DeleteFile`, `AdmitHeldFile` | single-row update | PK |
| User U/D log | history by user | `idx_transfers_user_started` |
| Rebuild user byte totals | `SUM(billable_bytes)` for completed transfers | `idx_transfers_user_day` / user scan |

## Concurrency

For 32+ concurrent BBS users:

- **Reads parallelize.** WAL gives every reader a consistent snapshot at
  the moment its transaction begins; concurrent SELECTs do not block one
  another and do not block the writer.
- **Writes serialize through the single writer connection.** At BBS
  cadence (transfer completions, status changes, occasional admin) this
  is far below SQLite's write throughput.
- **Same-user sessions compose through deltas.** Interactive BBS sessions
  and future FTP sessions may share a `user_id`. Transfer completion never
  writes a stale `User` snapshot back to SQLite; it applies additive
  deltas derived from one completed transfer.
- **Transfer completion is idempotent.** The writer transaction only
  applies accounting if it can claim an unfinished transfer row. Replayed
  completion calls return the already-recorded outcome without charging a
  second time.
- **Strict quota checks reserve capacity.** If a daily byte limit must be
  enforced across concurrent sessions, `BeginDownload` reserves billable
  bytes in the writer transaction. Without that reservation, limits are
  advisory under parallel FTP transfers.
- **`FlaggedFile` writes never hit SQLite** — they're per-session in
  memory, so the most frequent in-session "write" doesn't touch the
  database at all.
- **Extraction runs on a separate worker pool.** DIZ extraction and any
  future archive/AV checks live on a bounded `spawn_blocking` (or rayon)
  pool sized below CPU count and independent of the connection pool. One
  pathological `.tar.gz` cannot stall the Tokio reactor or starve other
  users; per-job timeouts and decompressed-byte ceilings cap the worst
  case.
- **Long-running read transactions are forbidden.** A reader that holds
  a transaction open across user input pins WAL growth. Read, materialise
  the result, release.

## Adapter layout

```
domain/files/
  file.rs              — File entity, FileStatus state machine, invariants
  flagged.rs           — FlaggedFiles/FlaggedKey (per-session set)
  flagged_store.rs     — FlaggedStore port + FlaggedStoreError (slice D5-persist)
  transfer.rs          — Transfer entity, outcome accounting
  repository.rs        — FileRepository, TransferLog ports
adapters/
  in_memory_flagged_store.rs  — Mutex<HashMap<u32, FlaggedFiles>> (default)
  sqlite_flagged_store.rs     — flagged_files table in users.db (durable, slice D5-persist)
adapters/files/
  sqlite_files.rs    — rusqlite + FTS5 trigram implementation
  fs_blob_store.rs   — blob read/write/move/delete on disk
  file_metadata.rs   — DIZ/comment extraction + content metadata
```

(`legacy_dir.rs`, the read-only ingest of AmiExpress DIR text files,
was dropped 2026-06-10: no legacy on-disk data compatibility — see
`slices/cmds-files-list.md`'s parity-target section.)

The `FileRepository` port stays narrow: methods named after the rules
that need them (`find_in_area`, `list_new_since`, `search_descriptions`,
`find_metadata`, `list_by_status`), not generic CRUD. Adapter is free to
add indexes for the queries the domain actually uses.

## Things to nail down

- ~~**Round-tripping the legacy DIR text format.**~~ Settled 2026-06-10:
  neither read nor write — no legacy data compatibility. Listings are
  generated at runtime from repository data; the DIR row *format*
  survives only as part of the rendered wire output (the AquaScan-style
  listing re-renders the same fields — see
  `comparison/evidence-tierD/live-observations.md`).
- **Full-text search scope.** v1 indexes short descriptions plus
  `FILE_ID.DIZ` / archive-comment metadata. Decide later whether full
  text/art file search is worth a separate content index.
- **`Transfer` retention.** The table grows unboundedly across years.
  Add a retention policy (or partition column) up front rather than
  after we hit 10M rows. If old transfer rows are pruned, persist a
  compacted baseline so user and membership projections can still be
  audited or rebuilt.
- **Open question from the spec:** `FlaggedFile` is per-session today
  but the legacy code persists it across sessions for the same user.
  If we follow the legacy behaviour, flagged files become a small
  durable table rather than session-only memory.
- **Extraction retry policy.** When DIZ extraction times out or aborts on
  budget exhaustion, do we leave the file with `file_id_diz = NULL`
  forever, expose a sysop-triggered re-extract, or schedule a single
  bounded retry? "Never retry" is the simplest default and matches
  best-effort semantics; revisit if operational logs show otherwise.
- **Worker pool sharing.** DIZ extraction and the spec's `BackgroundCheck`
  (AV / archive validation) have similar shapes: bounded, untrusted,
  CPU-bound. Decide whether they share one extraction worker pool or run
  on separate pools with their own budgets — sharing is simpler; splitting
  prevents a slow AV scan from delaying DIZ visibility.

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
