# Tier D (transfer) — Zmodem download / upload

Once browsing works ([cmds-files-list.md](cmds-files-list.md)), users
can see what's on the BBS but can't move it. These slices add the
Zmodem transfer adapter and the eligibility / accounting rules from
`files.allium`. Each transfer command (`D`, `U`, `RZ`) is its own
slice so partial value lands in turn.

See [SLICES.md](../SLICES.md) for the schema-growth principle and
asset inventory.

## Slice D-T1 — `Transfer` entity + Zmodem download adapter (stub)

- **In Scope**
  - `files.allium:Transfer` entity with `direction`, `started_at`,
    `bytes_transferred`, `cps`, `outcome`, `is_free_download`.
  - Zmodem **download** adapter that pulls bytes from a `File` and
    drives the protocol over the existing telnet stream
    (`amiexpress/zmodem.e` is the reference).
  - No accounting yet — the rule and adapter exist as a stub plumbed
    through `BeginDownload` / `TransferEnded`.
- **Out of Scope**
  - Xmodem, Ymodem, Hydra, FTP (deferred — pick Zmodem only).
  - Eligibility checks (Slice D-T3).

## Slice D-T2 — `D` (download a flagged file, no eligibility check)

- **In Scope**
  - Parser: `MenuCommand::Download(filenames)` — the legacy
    `internalCommandD` (`amiexpress/express.e:24853`) accepts a
    space-separated list, defaulting to the per-session flagged list
    when empty.
  - Adds `User.bytes_downloaded_total` (first read here).
  - `files.allium:BeginDownload`, `CompleteDownload` with the spec's
    accounting on the user, file and `ConferenceMembership`.
  - `is_free_download` honours `FileArea.free_downloads` and
    `Conference.free_downloads`.
- **Out of Scope**
  - Eligibility pre-flight (Slice D-T3).
- **Why ship now**: a user with the flag set can already download —
  the eligibility check is a refinement, not a prerequisite.

## Slice D-T3 — `CheckDownloadEligibility`

- **In Scope**
  - Adds `User.ratio_mode`, `User.ratio_value` (first read here).
  - `files.allium:CheckDownloadEligibility` — time,
    daily-byte cap, ratio (`ratio_check_passes`), credit-account
    bypass (`credit_account_active`).
  - `DailyDownloadsLeQuota` invariant.
  - Reports `DownloadEstimate` to the user before the transfer
    starts (legacy `beginDLF` confirmation block).
- **Out of Scope**
  - Free-resuming (`files.allium` open question).

## Slice D-T4a — `U` (upload, no time-bonus / no held-for-review)

- **In Scope**
  - Parser: `MenuCommand::Upload`.
  - Adds `User.bytes_uploaded_total` (first read here).
  - `files.allium:BeginUpload` allocates a `File` in `in_playpen`.
  - `files.allium:CompleteUpload` charges `bytes_uploaded_total`
    and transitions `in_playpen -> available` unconditionally.
  - `TransferBytesNonNegative` and `FileSizeNonNegative`
    invariants.
- **Out of Scope**
  - Time-bonus award (D-T4b).
  - `held_for_review` branch — dead until D-T6 lands a check, so
    deliberately omitted here.
- **Why split**: closes the headline "I uploaded a file" loop end-to-end
  in one TDD session.

## Slice D-T4b — Upload accounting refinements

- **In Scope**
  - Award `upload_time_bonus` (legacy `(bytes / 2 + 60)`
    seconds).
  - `held_for_review` branch path — activated together with D-T6's
    background check.

## Slice D-T5 — `RZ` (instant upload, resume Zmodem)

- **In Scope**
  - Parser: `MenuCommand::ResumeUpload`.
  - Wraps `uploadaFile(1, cmdcode, FALSE)` — the legacy "skip the
    description prompt, start receiving immediately" path
    (`amiexpress/express.e:25608`).
  - Reuses the Slice D-T4 entry points.
- **Out of Scope**
  - The `D-T6` background-check step (it inherits whatever D-T6 has
    landed by the time `RZ` ships).

## Slice D-T6 — Background file check

- **In Scope**
  - `files.allium:BackgroundCheck`, `BackgroundCheckPassed`,
    `BackgroundCheckFailed`.
  - Adapter reads per-archive-type config (reference
    `defaultbbs/FCheck/{DMS,EXE,LHA,LZH,LZX,ZIP}.info`) but stores
    config in TOML per `AGENTS.md`.
- **Out of Scope**
  - Anti-virus engines — the adapter shells out to a configured
    command.

## Slice D-T7 — `V` / `VS` (view a file in an archive)

- **In Scope**
  - Parser: `MenuCommand::ViewFile(filename)`.
  - Adapter integrates with the configured archive viewer
    (legacy `viewAFile` at `amiexpress/express.e:25682`) — for
    Zip / LhA / LZX, list contents and stream selected entry.
  - `VS` (`amiexpress/express.e:28376-28377`) is the same code path
    with the sysop-only `cmdcode` flag.
- **Out of Scope**
  - In-place archive editing.

## Slice D-T-wire — Tier D (transfer) wire-and-smoke (download)

- **In Scope**
  - End-to-end Zmodem download against the smoke client (use an
    embedded Zmodem implementation in the test harness so we don't
    shell out).
  - Validates that the byte accounting matches the spec invariants.
- **Out of Scope**
  - Upload smoke (slice D-T-wire-up below).

## Slice D-T-wire-up — Tier D (transfer) wire-and-smoke (upload)

- **In Scope**
  - End-to-end Zmodem upload against the smoke client: `U` triggers
    the receive, the embedded Zmodem client sends a fixture file,
    the file appears in the listing afterwards via `F`.
  - Same harness as D-T-wire, used in reverse.
  - Exercises the `held_for_review` branch from D-T4b once D-T6's
    background check has shipped.
- **Out of Scope**
  - Multi-file batch upload (legacy supports it via the file-card
    flow; landed once a sysop community signal asks for it).
