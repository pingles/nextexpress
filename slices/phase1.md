# Phase 1 — Sign in, see the menu, log off

This phase delivers the canonical "user can log in and out" loop. After
Slice 13, a sysop can configure a user, telnet in, see a menu, type `G`,
and disconnect cleanly.

See [SLICES.md](../SLICES.md) for the schema-growth principle, progress
table and asset inventory.

## Slice 2 — User entity (login-time fields only)
- **In Scope**
  - `User` entity (`core.allium:User`) trimmed to: `slot_number`, `handle`, `password_hash_kind`, `password_hash`, `password_salt`, `password_last_updated`, `access_level`.
  - Derived `is_sysop` (`slot_number == 1`).
  - `SaltMatchesAlgorithm` invariant enforced via constructor checks.
- **Out of Scope**
  - `invalid_attempts`, `account_locked` — added in Slice 11.
  - `force_password_reset` — added in Slice 15.
  - `is_new_user`, `censored` — added in Slices 20 / 47.
  - Time / byte / ratio / conference fields — each lands with the slice whose rule first reads it.
  - `is_locked_out` derived predicate — lands with Slice 16 once `account_locked` exists.

## Slice 3 — In-memory `UserRepository` port + adapter
- **In Scope**
  - `UserRepository` trait with `lookup_name(typed) -> NameLookupResult` and `user_for_name(handle) -> Option<User>` matching the black-box helpers in `session.allium`.
  - Wildcards rejected per `session.allium` guidance.
  - In-memory adapter seeded from a `Vec<User>`.
  - Tests cover `found`, `not_found`, and the literal `"NEW"` returning `user_typed_NEW`.
- **Out of Scope**
  - File-backed repository (deferred until disk format is firmed up later).

## Slice 4 — Password verification adapter (single algorithm)
- **In Scope**
  - `PasswordHashKind::pbkdf2_10000` only — the spec default for new accounts (`core.allium:config.password_hash_kind`).
  - `verify_password(user, candidate) -> bool` and `compute_password_hash(candidate, kind)` covering that one variant.
  - Rejects users whose `password_hash_kind` is anything else with a clear "unsupported hash kind" error so the gap is visible in tests.
- **Out of Scope**
  - Legacy 32-bit hash + `pbkdf2_5/50/100/1000` — Slice 64.
  - In-place rehash on stronger-algorithm logon — Slice 64.

## Slice 5 — Node entity (Phase 1 statuses only)
- **In Scope**
  - `Node` entity (`core.allium:Node`) with `number`, `status` and the subset of `Status` needed for the sign-in / log-off loop: `idle`, `connecting`, `logged_on`, `logging_off`.
  - The `idle -> connecting`, `connecting -> idle`, `connecting -> logged_on`, `logged_on -> logging_off`, `logging_off -> idle` transitions enforced.
- **Out of Scope**
  - `reserved` status + `reserved_for` field + `ReservedHasUser` invariant — Slice 24.
  - `suspended`, `shutting_down` — Slice 25.
  - `logging_on` intermediate state — added when a slice needs to distinguish it from `connecting`.
  - Multi-node concurrency (the BBS singleton runs nodes sequentially for now).

## Slice 6 — Session entity skeleton
- **In Scope**
  - `Session` entity (`session.allium:Session`) with the connection-time fields read by the Phase 1 rules: `node`, `channel`, `state`, `connected_at`, `last_input_at`, `online_baud`, `name_retry_count`, `password_retry_count`, `typed_name`, `user`, `authenticated_at`, `logoff_reason`, `logoff_at`.
  - The `state` transitions covering `connecting -> identifying -> authenticating -> onboarded -> menu -> logging_off -> ended` (the Phase 1 path).
  - `is_remote` and `is_authenticated` derived predicates.
  - `OneActiveSessionPerNode` and `SessionRetriesBounded` invariants.
- **Out of Scope**
  - The presentation booleans `ansi_colour`, `quick_logon`, `rip_mode`, `quiet_mode`, `cmd_shortcuts` — Slice 65.
  - `display_name_type` — Slice 34 (`JoinedConferenceForNameType`).
  - `time_remaining`, `bytes_remaining_today` — Slice 14.
  - `temp_access`, `reserved_for` — added with Slice 24.
  - `new_user_registering` state branch — Slice 19.

## Slice 7 — `AcceptConnection` rule
- **In Scope**
  - `session.allium:AcceptConnection` — creates a fresh `Session` in `connecting`, sets `connected_at`, `last_input_at`, `online_baud`, zero retry counters; flips node status to `connecting`.
  - Rejects when there is already a non-ended session for the node.
  - This slice introduces `core.allium:config.max_nodes` (the only config key it reads) with the spec default of `32`.
- **Out of Scope**
  - The boolean-flag `ensures` clauses on the rule (`ansi_colour: true`, `quick_logon: false`, `rip_mode: false`, `quiet_mode: false`, `cmd_shortcuts: false`, `display_name_type: handle`) — these fields don't exist yet; they're populated by their owning slices.
  - Wire-level transports — this slice tests the rule via direct invocation only.

## Slice 8 — Telnet adapter (await + banner)
- **In Scope**
  - Telnet listener with line-mode IAC negotiation (so the user sees what they type).
  - On accept, invoke `AcceptConnection` and write the BBSTITLE screen if a file exists at `bbs.path/Screens/BBSTITLE.txt`, otherwise a built-in fallback ("NextExpress\n").
  - End-to-end test: connect, read banner, drop connection.
- **Out of Scope**
  - ANSI / RIP / colour negotiation (Slice 65).
  - FTP (`session.allium:LogonChannel.ftp`) and serial transports.

## Slice 9 — `PromptForName` + `NameTyped` rules (existing user path only)
- **In Scope**
  - `session.allium:PromptForName` flips state `connecting -> identifying`.
  - `session.allium:NameTyped` for the `found` branch only (set `typed_name`, `user`, transition to `authenticating`).
  - `not_found` increments `name_retry_count`; after five strikes, end the session with `new_user_rejected`.
  - The literal `"NEW"` rejected with a "new users not yet supported" message — wired up in Slice 19.
- **Out of Scope**
  - `user_typed_NEW` registration flow (Slice 19).

## Slice 10 — `VerifyPassword` rule (happy path)
- **In Scope**
  - `session.allium:VerifyPassword` for matching credentials only: set `authenticated_at`, transition to `onboarded`.
  - End-to-end: telnet in, type handle, type correct password, observe state machine reaches `onboarded`.
- **Out of Scope**
  - Failure path, lockout, caller log entries (Slice 11).
  - `force_password_reset` follow-up (Slice 15).

## Slice 11 — `VerifyPassword` rule (failure path)
- **In Scope**
  - Adds `User.invalid_attempts` and `User.account_locked` fields plus the `LockoutClearsAttempts` invariant — these are first read by this rule.
  - Adds `core.allium:config.max_password_failures` (default `3`).
  - Wrong password increments `invalid_attempts` and `password_retry_count`.
  - `account_locked` set when `invalid_attempts >= max_password_failures`; session ends with `locked_account`.
  - Otherwise, after `max_password_failures` retries on this session, session ends with `excessive_password_fails`.
  - `CallerLog` entity (`session.allium:CallerLog`) and a caller-log appender adapter — entry created here with `is_password_failure: true`.
- **Out of Scope**
  - Email reset-code flow (`session.allium:VerifyPassword` `@guidance`).

## Slice 12 — `EnterMenu` + display the conference menu
- **In Scope**
  - Adds `User.times_called` and `User.last_call` fields (first read here).
  - `session.allium:EnterMenu` rule — increments `times_called`, transitions `onboarded -> menu`, writes a `CallerLog` line (`format_logon_line`).
  - Menu adapter writes the contents of `Conf02/Menu.txt` (bundled from `binaries.lha`) to the user, treating Amiga `\b\n` as `\r\n` and passing through ANSI escapes.
- **Out of Scope**
  - Per-conference / per-node menus (Slice 31).
  - Translator / multi-language (`core.allium` open question).

## Slice 13 — `UserRequestsLogoff` + `FinaliseLogoff` + `ReleaseNode`
- **In Scope**
  - The `G` command at the menu fires `LogoffRequested` (`session.allium:UserRequestsLogoff`).
  - `session.allium:FinaliseLogoff` updates `user.last_call`, writes the goodbye `CallerLog` line.
  - `session.allium:ReleaseNode` returns the node to `idle`.
  - Tests cover the full sign-in → menu → goodbye → ended path.
- **Out of Scope**
  - Accumulating `time_used_today` — Slice 14 introduces that field.
  - `relogon` re-entry (Slice 23).
  - `format_logoff_line`'s byte-tally fields (filled in once Phase 11 lands transfer accounting).
