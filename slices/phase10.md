# Phase 10 — Files (transfer)

Zmodem download / upload over telnet, with eligibility pre-flight and
background file checks for uploaded archives.

See [SLICES.md](../SLICES.md) for the schema-growth principle, progress
table and asset inventory.

## Slice 53 — `Transfer` entity + Zmodem adapter (download stub)
- **In Scope**
  - `files.allium:Transfer` entity with `direction`, `started_at`, `bytes_transferred`, `cps`, `outcome`, `is_free_download`.
  - Zmodem download adapter (`amiexpress/zmodem.e` is the reference) wired into `BeginDownload` / `TransferEnded`.
- **Out of Scope**
  - Xmodem, Ymodem, Hydra, FTP — pick zmodem only for this slice.

## Slice 54 — `BeginDownload` + `CompleteDownload`
- **In Scope**
  - Adds `User.bytes_downloaded_total` (first read here).
  - `files.allium:BeginDownload`, `CompleteDownload` rules with the spec's accounting on the user, file and `ConferenceMembership`.
  - `is_free_download` honours `FileArea.free_downloads` and `Conference.free_downloads`.
- **Out of Scope**
  - Pre-flight eligibility (Slice 55).

## Slice 55 — `CheckDownloadEligibility`
- **In Scope**
  - Adds `User.ratio_mode`, `User.ratio_value` (first read here).
  - `files.allium:CheckDownloadEligibility` — time, daily-byte cap, ratio (`ratio_check_passes`), credit-account bypass (`credit_account_active`).
  - `DailyDownloadsLeQuota` invariant.
  - Reports `DownloadEstimate` to the user.
- **Out of Scope**
  - Free-resuming (`files.allium` open question).

## Slice 56 — `BeginUpload` + `CompleteUpload`
- **In Scope**
  - Adds `User.bytes_uploaded_total` (first read here).
  - `files.allium:BeginUpload` allocates a `File` in `in_playpen`.
  - `files.allium:CompleteUpload` charges `bytes_uploaded_total`, awards `upload_time_bonus` (legacy `(bytes / 2 + 60) seconds`), transitions to `available` or `held_for_review`.
  - `TransferBytesNonNegative` and `FileSizeNonNegative` invariants.
- **Out of Scope**
  - Background check (Slice 57).

## Slice 57 — Background file check
- **In Scope**
  - `files.allium:BackgroundCheck`, `BackgroundCheckPassed`, `BackgroundCheckFailed`.
  - Adapter reads per-archive-type config (reference `defaultbbs/FCheck/{DMS,EXE,LHA,LZH,LZX,ZIP}.info`) but stores config in TOML per `AGENTS.md`.
- **Out of Scope**
  - Anti-virus engines — the adapter just shells out to a configured command.
