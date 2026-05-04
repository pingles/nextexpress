# Phase 2 — Hardening the logon flow

Adds the realities a logon system needs once the happy path works:
time-budgeting, password expiry, locked-account rejection, idle and
carrier-loss handling.

See [SLICES.md](../SLICES.md) for the schema-growth principle, progress
table and asset inventory.

## Slice 14 — Daily time budget initialisation + decrement
- **In Scope**
  - Adds `User.time_limit_per_call`, `User.time_limit_per_day`, `User.time_used_today`, `User.times_called_today` and `Session.time_remaining` (each first read here).
  - Adds `core.allium:config.daily_reset_offset` (default `6.hours`).
  - `session.allium:InitialiseDailyBudget` — daily counter rollover at `daily_reset_offset`; sets `time_remaining`.
  - `session.allium:UpdateTimeUsed` ticking each minute.
  - `session.allium:TimeExpired` forces logoff with `out_of_time`.
- **Out of Scope**
  - Daily byte cap (`User.daily_byte_limit`, `Session.bytes_remaining_today`) — Slice 63.
  - Chat-minute accounting — Slice 68.

## Slice 15 — Forced password reset
- **In Scope**
  - Adds `User.force_password_reset` field (first read here).
  - Adds `core.allium:config.password_expiry_days` (default `0`, meaning disabled).
  - `session.allium:ForcePasswordReset` — flag set when `password_expiry_days` expired or sysop has set `force_password_reset`.
  - `session.allium:CompletePasswordReset` — change-password sub-flow at `state = onboarded`, password-strength check (`meets_password_strength`).
  - Adds `core.allium:config.min_password_length` and `min_password_categories` for the strength check.
  - `EnterMenu` blocked while the flag is set (per the rule's `requires`).
- **Out of Scope**
  - In-place rehash on legacy → pbkdf2 algorithm change — Slice 64.

## Slice 16 — Account-locked / insufficient-access rejection
- **In Scope**
  - Adds the derived `User.is_locked_out` predicate (now that `account_locked` and `access_level` both exist).
  - `session.allium:RejectLockedOrInsufficientAccess` — bounce locked accounts, log to `CallerLog`, set `logoff_reason = locked_account` or `new_user_rejected`.
  - `LockedAccountsCannotEnterMenu` invariant covered by tests.
- **Out of Scope**
  - The "leave a comment for the sysop on the way out" branch (handled by the comment-to-sysop flow added in Slice 44).

## Slice 17 — Idle timeout
- **In Scope**
  - Adds `core.allium:config.input_timeout` (default `5.minutes`) and `treat_timeout_as_logoff` (default `false`).
  - `session.allium:IdleTimeout` — when `last_input_at + input_timeout <= now`, transition to `logging_off` with `input_timeout` or `carrier_loss` per `treat_timeout_as_logoff`.
  - Telnet adapter resets `last_input_at` on every input chunk.
- **Out of Scope**
  - Per-state timeout overrides.

## Slice 18 — Carrier loss
- **In Scope**
  - `session.allium:CarrierLost` — adapter's connection-closed event maps to `CarrierDropped(session)`; rule sets `logoff_reason = carrier_loss`.
  - Tests close the TCP socket mid-prompt and assert finalise + release.
- **Out of Scope**
  - Modem / serial CD; only telnet socket close in this phase.
