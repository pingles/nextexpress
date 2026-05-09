# Phase 12 — Per-user accounting refinements

Rounds out the accounting model: per-conference tallies, paid credit
accounts, the daily byte cap, and the long tail of password hash
algorithms with opportunistic in-place migration.

See [SLICES.md](../SLICES.md) for the schema-growth principle, progress
table and asset inventory.

## Slice 61 — Per-conference accounting
- **In Scope**
  - `ConferenceMembership.bytes_uploaded`, `bytes_downloaded`, `files_uploaded`, `files_downloaded`, `messages_posted` updated by the relevant rules.
  - Per-conference ratio override (`ConferenceMembership.ratio_mode != disabled` wins over user-level).
- **Out of Scope**
  - Reporting screens.

## Slice 62 — Credit accounts
- **In Scope**
  - `core.allium:CreditAccount` value attached to `User`.
  - `credit_account_active(user)` true within `start_date + days_credited`.
  - `track_uploads` / `track_downloads` toggle whether transfers count against the credit.
- **Out of Scope**
  - Payment / billing integration.

## Slice 63 — Daily byte cap end-to-end
- **In Scope**
  - Adds `User.daily_byte_limit`, `User.daily_bytes_downloaded`, `Session.bytes_remaining_today` (first read here).
  - `User.daily_byte_limit` enforced by `CheckDownloadEligibility` and `Session.bytes_remaining_today`.
  - Reset wired into `InitialiseDailyBudget`.
- **Out of Scope**
  - Per-conference daily caps (not in spec).

## Slice 64 — Legacy + lower-round password hashes
- **In Scope**
  - Adds the remaining `PasswordHashKind` variants (`legacy`, `pbkdf2_5`, `pbkdf2_50`, `pbkdf2_100`, `pbkdf2_1000`).
  - `verify_password` accepts all variants.
  - On a successful logon under a weaker algorithm, re-hash with the configured `password_hash_kind` and update the stored fields (the in-place migration mentioned in `core.allium:PasswordHashKind`).
  - Fixture vectors from `amiexpress/pwdhash.e` confirm parity with the legacy hash.
- **Out of Scope**
  - Forced bulk migration of stored passwords (out of scope; migration is opportunistic on logon).
