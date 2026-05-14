# User Storage Design

How NextExpress persists user accounts and serves them under concurrent
load. Pairs with [`specs/core.allium`](../specs/core.allium), which
defines the `User`, `ConferenceMembership`, and `ReadPointers` entities.

## Scope

Durable storage and access patterns for:

- The `User` entity (identity, auth state, ratios, time/byte accounting,
  display preferences, credit account).
- `ConferenceMembership` (per-user, per-conference grant + accounting).
- `ReadPointers` (per-user, per-msgbase read state).

Out of scope:

- Session lifecycle (`specs/session.allium`).
- Password hashing algorithms — already covered by
  `domain/password.rs` and `adapters/pbkdf2_password_hasher.rs`.
- Caller log — separate concern (`domain/caller_log.rs`).

## Sizing

A typical BBS has hundreds to a few thousand users; large boards reach
tens of thousands. Each `User` row serialises to roughly 1–2 KB across
all its fields. Even 50K users is ~100 MB on disk and well within
SQLite's comfort zone.

Membership and read-pointer rows scale as `users × conferences` and
`users × msgbases` respectively — still small absolute numbers given
typical BBS conference counts.

## Decisions

### SQLite as the single source of truth

Same posture as [FILES.md](./FILES.md):

- **WAL mode**, `synchronous = NORMAL`, `busy_timeout` of a few
  seconds, prepared statements.
- Async-friendly connection pool (`tokio-rusqlite` or
  `deadpool-sqlite`), one read pool plus a dedicated writer.
- Foreign keys enabled.

### No in-memory user cache in v1

The user-storage access pattern is dominated by a session reading and
writing **its own** user record. This is not a high-concurrency hot
path:

- Login: one lookup by handle, one password verify. Once per session.
- Per-session reads: fields like `ratio_mode`, `time_remaining_today`,
  `daily_byte_limit`. SQLite point lookups at 10–50 µs apiece are well
  below the latency floor of a telnet round-trip.
- Per-session writes: counter bumps on download completion, time used
  on tick, `last_call`/`times_called` at session end. All single-row
  updates against the writer connection.

Cross-session reads (sysop "list users", "who has access to X") are rare
and tolerate millisecond latency.

A `DashMap`-of-`UserCell` layer on top would buy nothing measurable at
this scale and would introduce cache coherency questions (sysop edits
during an active session, multi-node logins for the same user). Don't
build it speculatively.

### Write policy by field

Not all writes are equally urgent. The adapter exposes both immediate
and deferred write paths so the domain can choose:

| Field | When to flush | Why |
|---|---|---|
| `password_hash`, `password_hash_kind`, `password_salt`, `password_last_updated` | Immediate, `synchronous = FULL` for this commit | Security boundary; must survive crash |
| `account_locked`, `invalid_attempts`, `force_password_reset` | Immediate | Lockout policy depends on durability |
| `access_level`, `is_new_user`, `censored` | Immediate | Authorisation decisions |
| `bytes_downloaded_total`, `daily_bytes_downloaded` | On `CompleteDownload` | Ratio enforcement on next login depends on it |
| `bytes_uploaded_total` | On `CompleteUpload` | Same |
| `time_used_today`, `time_remaining_today` | On session tick or end-of-session | Cheap, but not security-critical |
| `last_call`, `times_called`, `times_called_today` | End-of-session | One write per session is enough |
| Display prefs (`expert_mode`, `line_length`, `ansi_colour`, `flags`, …) | End-of-session | Cosmetic |
| `last_joined_conference`, `last_joined_msgbase` | End-of-session | Restored on next login |

A small in-process **session-end flush queue** batches the deferred
writes into one transaction at logoff. Crash before flush loses at most
one session's cosmetic state.

### Login lookup goes through SQLite, not a cached index

Login frequency is bounded by humans dialling in. A `SELECT` by a
case-folded handle column with a unique index resolves in tens of
microseconds. No need for an `ArcSwap<HashMap<NormalizedHandle, UserId>>`
unless profiling later proves otherwise.

Store the case-folded form alongside the original to keep the lookup
index simple:

```sql
handle           TEXT NOT NULL,
handle_folded    TEXT NOT NULL UNIQUE
```

## Schema sketch

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;

CREATE TABLE users (
    id                          INTEGER PRIMARY KEY,
    slot_number                 INTEGER NOT NULL UNIQUE,   -- 1 = sysop
    handle                      TEXT    NOT NULL,
    handle_folded               TEXT    NOT NULL UNIQUE,
    real_name                   TEXT,
    internet_name               TEXT,
    location                    TEXT,
    phone_number                TEXT,
    email                       TEXT,

    password_hash_kind          TEXT    NOT NULL,
    password_hash               TEXT    NOT NULL,
    password_salt               TEXT,
    password_last_updated       INTEGER NOT NULL,
    invalid_attempts            INTEGER NOT NULL DEFAULT 0,
    force_password_reset        INTEGER NOT NULL DEFAULT 0,
    account_locked              INTEGER NOT NULL DEFAULT 0,

    access_level                INTEGER NOT NULL,
    is_new_user                 INTEGER NOT NULL DEFAULT 1,
    censored                    INTEGER NOT NULL DEFAULT 0,

    ratio_mode                  TEXT    NOT NULL,          -- RatioMode enum
    ratio_value                 INTEGER NOT NULL DEFAULT 0,

    time_limit_per_call_secs    INTEGER NOT NULL DEFAULT 0,
    time_limit_per_day_secs     INTEGER NOT NULL DEFAULT 0,
    time_used_today_secs        INTEGER NOT NULL DEFAULT 0,
    times_called                INTEGER NOT NULL DEFAULT 0,
    times_called_today          INTEGER NOT NULL DEFAULT 0,
    last_call                   INTEGER,
    account_created             INTEGER NOT NULL,

    bytes_downloaded_total      INTEGER NOT NULL DEFAULT 0,
    bytes_uploaded_total        INTEGER NOT NULL DEFAULT 0,
    daily_byte_limit            INTEGER NOT NULL DEFAULT 0,
    daily_bytes_downloaded      INTEGER NOT NULL DEFAULT 0,

    messages_posted             INTEGER NOT NULL DEFAULT 0,

    last_joined_conference_id   INTEGER REFERENCES conferences(id),
    last_joined_msgbase_id      INTEGER REFERENCES msgbases(id),

    chat_minutes_remaining_secs INTEGER NOT NULL DEFAULT 0,
    chat_minutes_per_call_secs  INTEGER NOT NULL DEFAULT 0,

    expert_mode                 INTEGER NOT NULL DEFAULT 0,
    line_length                 INTEGER NOT NULL DEFAULT 0,
    ansi_colour                 INTEGER NOT NULL DEFAULT 1,
    preferred_protocol          TEXT    NOT NULL DEFAULT 'zmodem',
    flags                       INTEGER NOT NULL DEFAULT 0,  -- bitmask of UserFlag

    -- Embedded credit account (nullable group).
    credit_days                 INTEGER,
    credit_amount_paid          REAL,
    credit_start_date           INTEGER,
    credit_total_paid_to_date   REAL,
    credit_last_total_paid_date INTEGER,
    credit_track_uploads        INTEGER,
    credit_track_downloads      INTEGER
);

CREATE INDEX idx_users_slot      ON users (slot_number);
CREATE INDEX idx_users_last_call ON users (last_call);

CREATE TABLE conference_memberships (
    id                  INTEGER PRIMARY KEY,
    user_id             INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    conference_id       INTEGER NOT NULL REFERENCES conferences(id),
    granted             INTEGER NOT NULL DEFAULT 1,
    bytes_uploaded      INTEGER NOT NULL DEFAULT 0,
    bytes_downloaded    INTEGER NOT NULL DEFAULT 0,
    files_uploaded      INTEGER NOT NULL DEFAULT 0,
    files_downloaded    INTEGER NOT NULL DEFAULT 0,
    messages_posted     INTEGER NOT NULL DEFAULT 0,
    ratio_mode          TEXT    NOT NULL DEFAULT 'disabled',
    ratio_value         INTEGER NOT NULL DEFAULT 0,
    UNIQUE (user_id, conference_id)
);

CREATE TABLE read_pointers (
    id              INTEGER PRIMARY KEY,
    membership_id   INTEGER NOT NULL REFERENCES conference_memberships(id) ON DELETE CASCADE,
    msgbase_id      INTEGER NOT NULL REFERENCES msgbases(id),
    last_read       INTEGER NOT NULL DEFAULT 0,
    last_scanned    INTEGER NOT NULL DEFAULT 0,
    new_since       INTEGER NOT NULL DEFAULT 0,
    UNIQUE (membership_id, msgbase_id),
    CHECK (last_read <= last_scanned)
);

-- Append-only audit trail for security-relevant events.
CREATE TABLE user_events (
    id          INTEGER PRIMARY KEY,
    user_id     INTEGER REFERENCES users(id),  -- nullable for failed logins by unknown handle
    at          INTEGER NOT NULL,
    kind        TEXT    NOT NULL,              -- 'login_ok' | 'login_fail' | 'lockout' | 'pw_change' | 'sysop_edit' | …
    detail      TEXT
);

CREATE INDEX idx_user_events_user_at ON user_events (user_id, at DESC);
```

## Access patterns and how they map

| Operation | Query | Index used |
|---|---|---|
| Login (handle → user) | `SELECT … WHERE handle_folded = ?` | `UNIQUE (handle_folded)` |
| Sysop reference by slot | `SELECT … WHERE slot_number = ?` | `idx_users_slot` |
| Per-session reads | `SELECT … WHERE id = ?` | PK |
| Counter updates | `UPDATE users SET … WHERE id = ?` | PK |
| Sysop list users | paginated `SELECT … ORDER BY slot_number` | full scan or `idx_users_slot` |
| Daily reset | `UPDATE users SET daily_bytes_downloaded = 0, time_used_today_secs = 0, times_called_today = 0` | full scan, runs once per user on first login of day |
| Membership lookup for ratios / accounting | `SELECT … WHERE user_id = ? AND conference_id = ?` | `UNIQUE (user_id, conference_id)` |
| Mail scan | `SELECT … WHERE membership_id = ? AND msgbase_id = ?` | `UNIQUE (membership_id, msgbase_id)` |
| Audit lookup for an account | `SELECT … FROM user_events WHERE user_id = ? ORDER BY at DESC` | `idx_user_events_user_at` |

## Concurrency

- **One session per user is the common case.** Per-session reads and
  writes against the writer connection serialise naturally.
- **Two sessions for the same user is allowed by the spec.** Both go
  through the same writer connection; SQLite serialises writes, so
  byte-counter updates from concurrent downloads compose correctly
  (each is `UPDATE … SET col = col + ?`). Reads see a consistent
  snapshot per transaction.
- **Sysop edits during an active session** land in SQLite immediately.
  The owning session reads the new values on its next query — no
  invalidation protocol needed because there is no cache to invalidate.
- **Daily reset** is one `UPDATE` against all users. Cheap, runs in a
  single writer transaction.

## Adapter layout

```
domain/
  user.rs                — User entity (already exists)
  user_repository.rs     — UserRepository port (already exists)
adapters/
  in_memory_user_repository.rs    — kept for tests
  sqlite_user_repository.rs       — new: rusqlite, write-policy aware
```

The existing `InMemoryUserRepository` stays as the test double; the
SQLite implementation is the production adapter. The
`UserRepository` port grows methods that reflect the write-policy
distinction:

- `save_security_state(&User)` — immediate, `synchronous = FULL`
- `save_session_counters(&User)` — immediate, normal sync
- `save_session_end(&User)` — batched at logoff
- `find_by_handle(&str)`, `find_by_id(UserId)`, `find_by_slot(u32)`,
  `list(...)` — reads

## Things to nail down

- **Concurrent same-user logins.** Spec doesn't forbid them. Decide
  whether to refuse the second login, displace the first, or allow
  both. Allowing both is straightforward at the storage layer (writes
  serialise), but has user-visible implications (whose `last_call`
  wins? do byte counters race?). Default suggestion: allow, document,
  let counter `UPDATE … SET col = col + ?` handle the arithmetic.
- **Audit log retention.** `user_events` will grow without bound. Add a
  rolling window (e.g. 1 year) or let the sysop prune.
- **Schema migrations.** Pick an approach now (`refinery`, hand-rolled
  `PRAGMA user_version`, `sqlx migrate`) so the first SQLite-backed
  release ships with migration tooling rather than retrofitting it.
- **Migration from the legacy on-disk user files.** A one-shot
  importer reads the legacy `user`, `userKeys`, `userMisc` files (per
  `core.allium` docstring) and inserts rows. Owned by a separate
  ingest tool, not part of the runtime adapter.

## Future optimisations (not for v1)

Add only when measurement says so:

- **`DashMap<UserId, Arc<UserCell>>` cache** with per-cell `RwLock`,
  for the case where per-session counter writes ever dominate a CPU
  profile. At expected BBS cadence, they won't.
- **`ArcSwap<HashMap<NormalizedHandle, UserId>>`** login index, if
  login latency becomes user-visible (it won't at human dial-in
  rates).
- **Read replicas** — irrelevant unless we ever go multi-process.
