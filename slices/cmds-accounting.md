# Tier I — Accounting + crypto refinements

Cross-cutting refinements that don't introduce new user-visible
commands but extend the rules that the commands of Tiers D and F
*depend on*. These slices land once the file-transfer surface has
shipped and we have evidence of which accounting paths users
actually hit.

See [SLICES.md](../SLICES.md) for the schema-growth principle.

## Slice I1 — Per-conference accounting

- **In Scope**
  - `ConferenceMembership.bytes_uploaded`,
    `bytes_downloaded`, `files_uploaded`, `files_downloaded`,
    `messages_posted` updated by the relevant rules.
  - Per-conference ratio override
    (`ConferenceMembership.ratio_mode != disabled` wins over
    user-level).
- **Out of Scope**
  - Reporting screens — they're rendered by Tier A's `S` slice
    when these fields exist.

## Slice I2 — Credit accounts

- **In Scope**
  - `core.allium:CreditAccount` value attached to `User`.
  - `credit_account_active(user)` true within
    `start_date + days_credited`.
  - `track_uploads` / `track_downloads` toggle whether transfers
    count against the credit.
- **Out of Scope**
  - Payment / billing integration.

## Slice I3 — Daily byte cap end-to-end

- **In Scope**
  - Adds `User.daily_byte_limit`, `User.daily_bytes_downloaded`,
    `Session.bytes_remaining_today` (first read here).
  - `User.daily_byte_limit` enforced by
    `CheckDownloadEligibility` and
    `Session.bytes_remaining_today`.
  - Reset wired into `InitialiseDailyBudget`.
  - Tier A's `T`-with-time-remaining display picks up the new
    `bytes_remaining_today` line.
- **Out of Scope**
  - Per-conference daily caps (not in spec).

## Slice I3b — Low-credit weighting in download ratio formula

- **In Scope**
  - Parameterises the existing `ratio_check_passes` predicate so an
    `lcfiles` file (the legacy "low-credit" state from
    [cmds-files-sysop.md](cmds-files-sysop.md)'s D-S3) weighs less
    against the ratio than a normal `available` file.
  - Weighting factor lives in `core.allium:Config.lcfiles_weight`
    (new key, defaults to `0.5` per legacy norm).
- **Depends on**: D-S3 (`lcfiles` status) and I1 (the
  per-conference ratio path that this slice extends).
- **Out of Scope**
  - Variable weighting per area — single global factor.

## Slice I4 — Legacy + lower-round password hashes

- **In Scope**
  - Adds the remaining `PasswordHashKind` variants
    (`legacy`, `pbkdf2_5`, `pbkdf2_50`, `pbkdf2_100`,
    `pbkdf2_1000`).
  - `verify_password` accepts all variants.
  - On a successful logon under a weaker algorithm, re-hash with
    the configured `password_hash_kind` and update the stored
    fields (the in-place migration mentioned in
    `core.allium:PasswordHashKind`).
  - Fixture vectors from `amiexpress/pwdhash.e` confirm parity
    with the legacy hash.
- **Out of Scope**
  - Forced bulk migration of stored passwords — migration is
    opportunistic on logon.
