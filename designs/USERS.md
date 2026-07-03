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
issuing narrow updates against its associated user id. This is not a
high-concurrency hot path:

- Login: one lookup by handle, one password verify. Once per session.
- Per-session reads: fields like `ratio_mode`, `time_remaining_today`,
  `daily_byte_limit`. SQLite point lookups at 10–50 µs apiece are well
  below the latency floor of a telnet round-trip.
- Per-session writes: counter bumps on download completion, time used
  on tick, `last_call`/`times_called` at session end. These are narrow
  single-row updates against the writer connection.

Cross-session reads (sysop "list users", "who has access to X") are rare
and tolerate millisecond latency.

A `DashMap`-of-`UserCell` layer on top would buy nothing measurable at
this scale and would introduce cache coherency questions (sysop edits
during an active session, multi-node logins for the same user). Don't
build it speculatively.

### Multiple sessions per user are supported

> **Landed (2026-07-03), SYSTEM.md item 1.** The command-style writes
> below exist: `record_auth_outcome` (verify-password),
> `record_password_change` (forced reset), and `apply_user_patch`
> (menu entry + logoff — additive counters, `MAX`-merged
> `last_call`/read pointers, last-writer-wins preference patches,
> each command one SQL transaction). The whole-aggregate
> `UserRepository::save` is deleted. Still future work from this
> design: the session-end flush queue and the per-field
> immediate/deferred split (no consumer yet — sessions flush at their
> three natural persist points), and the transfer-byte
> reservation/delta columns (land with D-T2).

Future FTPD support makes same-user concurrency a normal case: a user may
have an interactive BBS session and one or more FTP transfer sessions
open at the same time. Storage must therefore treat the session's `User`
value as a snapshot for presentation and local policy decisions, not as an
owned record that can be saved wholesale.

The adapter exposes command-style writes:

- **Additive counters** use deltas (`SET col = col + ?`) for byte totals,
  time used, call counts, message counts, and membership accounting.
- **Monotonic pointers/timestamps** use guarded writes such as
  `MAX(existing, new)` where moving backwards would be wrong.
- **Security and authorisation fields** are immediate authoritative
  writes and are re-read before security-sensitive operations.
- **Preferences** are field patches. Last-writer-wins is acceptable for
  cosmetic fields, but broad whole-row session saves are forbidden.

This keeps concurrent sessions commutative where possible. SQLite's
single writer serialises the commands; the command shape ensures the
later command composes with the earlier one instead of overwriting it.

### Write policy by field

Not all writes are equally urgent. The adapter exposes both immediate
and deferred write paths so the domain can choose:

| Field | When to flush | Why |
|---|---|---|
| `password_hash`, `password_hash_kind`, `password_salt`, `password_last_updated` | Immediate, `synchronous = FULL` for this commit | Security boundary; must survive crash |
| `account_locked`, `invalid_attempts`, `force_password_reset` | Immediate | Lockout policy depends on durability |
| `access_level`, `is_new_user`, `censored` | Immediate | Authorisation decisions |
| `bytes_downloaded_total`, `daily_bytes_downloaded` | Additive delta from completed transfer | Ratio enforcement and FTPD concurrency |
| `daily_bytes_reserved` | Reserve/release around in-flight downloads | Strict daily caps across concurrent sessions |
| `bytes_uploaded_total` | Additive delta from completed transfer | Same |
| `time_used_today` | Add elapsed delta on tick or end-of-session | Concurrent sessions compose |
| `times_called`, `times_called_today` | Additive delta at logon/menu entry | One increment per successful call |
| `last_call` | Monotonic `MAX(last_call, at)` at logoff | Older session ending late must not move it backwards |
| Display prefs (`expert_mode`, `line_length`, `ansi_colour`, `flags`, …) | End-of-session patch | Cosmetic; last writer wins unless versioning is needed |
| `last_joined_conference`, `last_joined_msgbase` | End-of-session | Restored on next login |

A small in-process **session-end flush queue** batches the deferred
writes into one transaction at logoff. The queue stores patches and
deltas, never a full `User` snapshot. Crash before flush loses at most
one session's cosmetic state and unflushed time delta.

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
    daily_accounting_day        INTEGER NOT NULL DEFAULT 0,
    times_called                INTEGER NOT NULL DEFAULT 0,
    times_called_today          INTEGER NOT NULL DEFAULT 0,
    last_call                   INTEGER,
    account_created             INTEGER NOT NULL,

    bytes_downloaded_total      INTEGER NOT NULL DEFAULT 0,
    bytes_uploaded_total        INTEGER NOT NULL DEFAULT 0,
    daily_byte_limit            INTEGER NOT NULL DEFAULT 0,
    daily_bytes_downloaded      INTEGER NOT NULL DEFAULT 0,
    daily_bytes_reserved        INTEGER NOT NULL DEFAULT 0,

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
    preferences_version         INTEGER NOT NULL DEFAULT 0,

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
| Counter updates | `UPDATE users SET col = col + ? WHERE id = ?` | PK |
| Daily byte reservation | guarded `UPDATE ... SET daily_bytes_reserved = daily_bytes_reserved + ?` | PK |
| Sysop list users | paginated `SELECT … ORDER BY slot_number` | full scan or `idx_users_slot` |
| Daily reset | guarded reset by `daily_accounting_day` | PK per user, idempotent |
| Membership lookup for ratios / accounting | `SELECT … WHERE user_id = ? AND conference_id = ?` | `UNIQUE (user_id, conference_id)` |
| Mail scan | `SELECT … WHERE membership_id = ? AND msgbase_id = ?` | `UNIQUE (membership_id, msgbase_id)` |
| Audit lookup for an account | `SELECT … FROM user_events WHERE user_id = ? ORDER BY at DESC` | `idx_user_events_user_at` |

## Concurrency

- **Multiple sessions for the same user are supported.** This includes an
  interactive session plus future FTP sessions. Per-session reads may use
  a snapshot, but persistent writes must be deltas or field patches.
- **Whole-user session saves are forbidden.** A session must not write a
  hydrated `User` back to SQLite at logoff. That would let an older
  session overwrite a newer session's counters, sysop edits, or FTP
  accounting.
- **Additive updates compose.** Byte totals, time used, call counts,
  message counts, and membership counters use `UPDATE … SET col = col + ?`.
  SQLite serialises the writer transactions; the arithmetic is
  commutative across sessions.
- **Daily reset is guarded.** The reset uses `daily_accounting_day` so two
  sessions crossing the same day boundary cannot both reset counters after
  the other has started adding usage.
- **Strict daily byte limits use reservations.** A transfer reserves
  billable bytes before it starts and releases or converts the reservation
  at completion. Without this, parallel FTP transfers could all pass the
  same stale eligibility check.
- **Sysop edits during an active session** land in SQLite immediately.
  Security-sensitive operations re-read current authorisation state before
  acting; no invalidation protocol is needed because there is no cache to
  invalidate.

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

- `save_security_state(SecurityPatch)` — immediate, `synchronous = FULL`
- `reset_daily_if_needed(user_id, accounting_day)` — guarded/idempotent
- `record_logon(user_id, session_id, at, accounting_day)` — additive call counters
- `add_time_used(user_id, session_id, elapsed)` — additive time delta
- `reserve_daily_download_bytes(user_id, transfer_id, accounting_day, bytes)` — guarded reservation
- `release_daily_download_reservation(user_id, transfer_id, bytes)` — additive negative reservation
- `apply_transfer_accounting(delta)` — additive projection update from one completed transfer
- `advance_read_pointer(user_id, msgbase_id, pointer)` — monotonic pointer update
- `save_preferences_patch(user_id, patch)` — field patch, optionally version checked
- `find_by_handle(&str)`, `find_by_id(UserId)`, `find_by_slot(u32)`,
  `list(...)` — reads

## Things to nail down

- **Preference conflict UX.** Last-writer-wins is safe for cosmetic
  preferences, but the sysop editor may want optimistic version checks so
  it can warn before overwriting an active session's preference patch.
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
