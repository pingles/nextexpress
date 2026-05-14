# File Storage Design

How NextExpress persists file-area metadata and serves it under concurrent
load. Pairs with [`specs/files.allium`](../specs/files.allium), which
defines the behaviour this design supports.

## Scope

This document covers durable storage and search for the `File`, `Transfer`,
and `FlaggedFile` entities, plus the access patterns the rules in
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

### Connection strategy

- An async-friendly pool such as `tokio-rusqlite` (worker thread per
  connection) or `deadpool-sqlite`.
- Sized to roughly the CPU core count for read connections, plus one
  dedicated writer connection. With WAL, readers proceed in parallel.
- One connection per task; never share a connection across awaits.

### FTS5 with the trigram tokenizer for search

The legacy `Z` (zippy) command is substring search across file
descriptions in one or more areas. SQLite's FTS5 with the **`trigram`
tokenizer** gives us substring `MATCH` queries directly:

```sql
SELECT file_id
FROM files_fts
WHERE files_fts MATCH ?
  AND area_id IN (...);
```

Substring "graph" decomposes into trigrams `gra`, `rap`, `aph`; FTS5
intersects the posting lists and verifies candidates. Index size is on
the order of 1.5× the description text (~6 MB for 20K descriptions).
Search times for our scale are sub-millisecond to a few ms.

### `FlaggedFile` is per-session, in-memory only

- Lives on the session struct, capped at `max_flagged_files()` (1000).
- Dies with the session.
- No SQLite involvement; no need for durability.
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
    uploaded_by     INTEGER REFERENCES users(id),
    uploaded_at     INTEGER NOT NULL,         -- epoch seconds
    last_downloaded_at INTEGER,
    download_count  INTEGER NOT NULL DEFAULT 0,
    UNIQUE (area_id, name)
);

CREATE INDEX idx_files_area_uploaded_at  ON files (area_id, uploaded_at DESC);
CREATE INDEX idx_files_status_pending    ON files (status)
    WHERE status IN ('held_for_review', 'quarantined', 'in_playpen');

-- Trigram FTS for the zippy search.
CREATE VIRTUAL TABLE files_fts USING fts5 (
    description,
    content      = 'files',
    content_rowid = 'id',
    tokenize     = 'trigram'
);

-- Keep FTS in sync with the canonical table.
CREATE TRIGGER files_ai AFTER INSERT ON files BEGIN
    INSERT INTO files_fts(rowid, description) VALUES (new.id, new.description);
END;
CREATE TRIGGER files_ad AFTER DELETE ON files BEGIN
    INSERT INTO files_fts(files_fts, rowid, description)
    VALUES ('delete', old.id, old.description);
END;
CREATE TRIGGER files_au AFTER UPDATE OF description ON files BEGIN
    INSERT INTO files_fts(files_fts, rowid, description)
    VALUES ('delete', old.id, old.description);
    INSERT INTO files_fts(rowid, description) VALUES (new.id, new.description);
END;

CREATE TABLE transfers (
    id              INTEGER PRIMARY KEY,
    session_id      INTEGER NOT NULL,
    user_id         INTEGER NOT NULL REFERENCES users(id),
    file_id         INTEGER NOT NULL REFERENCES files(id),
    direction       TEXT    NOT NULL,         -- 'upload' | 'download'
    started_at      INTEGER NOT NULL,
    finished_at     INTEGER,
    bytes_transferred INTEGER NOT NULL DEFAULT 0,
    cps             INTEGER NOT NULL DEFAULT 0,
    outcome         TEXT,                     -- TransferOutcome enum, null until finished
    is_free_download INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_transfers_user_started ON transfers (user_id, started_at DESC);
CREATE INDEX idx_transfers_file_started ON transfers (file_id, started_at DESC);
```

## Access patterns and how they map

| Spec rule | Query | Index used |
|---|---|---|
| `FlagFile` | lookup by `(area_id, name)` | `UNIQUE (area_id, name)` |
| `displayFileList` | list by area, often "since X" | `idx_files_area_uploaded_at` |
| `zippy` substring search | `files_fts MATCH ?` filtered by area set | trigram FTS |
| `CheckDownloadEligibility` | sum `size_bytes` for ≤1000 ids | PK lookups; small N |
| `BeginDownload` / `BeginUpload` | insert `transfers`, lookup `files` row | PK + writer connection |
| `CompleteDownload` | update file counters + insert nothing new | PK |
| `CompleteUpload` | update file row + counters | PK |
| Sysop "held files" | list by status | `idx_files_status_pending` |
| `MoveFile`, `DeleteFile`, `AdmitHeldFile` | single-row update | PK |
| User U/D log | history by user | `idx_transfers_user_started` |

## Concurrency

For 32+ concurrent BBS users:

- **Reads parallelize.** WAL gives every reader a consistent snapshot at
  the moment its transaction begins; concurrent SELECTs do not block one
  another and do not block the writer.
- **Writes serialize through the single writer connection.** At BBS
  cadence (transfer completions, status changes, occasional admin) this
  is far below SQLite's write throughput.
- **`FlaggedFile` writes never hit SQLite** — they're per-session in
  memory, so the most frequent in-session "write" doesn't touch the
  database at all.
- **Long-running read transactions are forbidden.** A reader that holds
  a transaction open across user input pins WAL growth. Read, materialise
  the result, release.

## Adapter layout

```
domain/files/
  file.rs            — File entity, FileStatus state machine, invariants
  flagged.rs         — FlaggedFile (per-session, in-memory)
  transfer.rs        — Transfer entity, outcome accounting
  repository.rs      — FileRepository, TransferLog ports
adapters/files/
  sqlite_files.rs    — rusqlite + FTS5 trigram implementation
  fs_blob_store.rs   — blob read/write/move/delete on disk
  legacy_dir.rs      — read-only ingest of AmiExpress DIR text files
```

The `FileRepository` port stays narrow: methods named after the rules
that need them (`find_in_area`, `list_new_since`, `search_descriptions`,
`list_by_status`), not generic CRUD. Adapter is free to add indexes for
the queries the domain actually uses.

## Things to nail down

- **Round-tripping the legacy DIR text format.** Does the port need to
  *write* DIR files for backward compatibility, or only *read* them as a
  one-shot ingest? Affects whether `legacy_dir.rs` needs a serialiser.
- **`Transfer` retention.** The table grows unboundedly across years.
  Add a retention policy (or partition column) up front rather than
  after we hit 10M rows.
- **Open question from the spec:** `FlaggedFile` is per-session today
  but the legacy code persists it across sessions for the same user.
  If we follow the legacy behaviour, flagged files become a small
  durable table rather than session-only memory.

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
