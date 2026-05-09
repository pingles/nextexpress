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
  - A `NodePool` holding `config.max_nodes` nodes, allocating an idle one atomically (the supervisor will use this from Slice 8). Tests prove two concurrent allocations claim distinct nodes.
- **Out of Scope**
  - `reserved` status + `reserved_for` field + `ReservedHasUser` invariant — Slice 24.
  - `suspended`, `shutting_down` — Slice 25.
  - `logging_on` intermediate state — added when a slice needs to distinguish it from `connecting`.

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
  - Rejects when there is already a non-ended session for the node. Runs against an already-locked node from the `NodePool`, so the "no other session on this node" check is local; concurrency safety is the pool's job.
  - This slice introduces `core.allium:config.max_nodes` (the only config key it reads) with the spec default of `32`.
- **Out of Scope**
  - The boolean-flag `ensures` clauses on the rule (`ansi_colour: true`, `quick_logon: false`, `rip_mode: false`, `quiet_mode: false`, `cmd_shortcuts: false`, `display_name_type: handle`) — these fields don't exist yet; they're populated by their owning slices.
  - Wire-level transports — this slice tests the rule via direct invocation only.

## Slice 8 — Telnet listener + per-session task
- **In Scope**
  - Async telnet listener (`tokio::net::TcpListener`) with line-mode IAC negotiation (advertises `WILL ECHO`, `WILL SUPPRESS-GO-AHEAD`, `DO SUPPRESS-GO-AHEAD`). Note that advertising `WILL ECHO` creates an obligation to actually echo, which Slice 8a discharges.
  - For each accepted TCP connection: try to allocate a node from the `NodePool` (Slice 5); if all `config.max_nodes` are in use, send a "BBS busy" line and close. On success, spawn a `tokio::task` that owns the connection for its lifetime and invokes `AcceptConnection` (Slice 7).
  - The session task writes the BBSTITLE screen if a file exists at `bbs.path/Screens/BBSTITLE.txt`, otherwise a built-in fallback ("NextExpress\n"), then drops.
  - Concurrent end-to-end test: open `max_nodes + 1` simultaneous connections, assert that the first `max_nodes` each see the banner and the surplus is rejected. Assert that closing one frees its node so a fresh connection can grab it.
- **Out of Scope**
  - Wire-quality (echo, password masking, line editing, source-faithful prompts) — Slice 8a.
  - ANSI / RIP / colour negotiation (Slice 65).
  - SSH and FTP transports — see [`future.md`](future.md).
  - A `Transport` trait — extracted when a second transport adapter lands.
  - Modem / serial CD.

## Slice 8a — Telnet wire-quality (echo, password masking, line editing, AmiExpress prompts)
- **In Scope**
  - Discharges every item on the **Wire-quality checklist** in [SLICES.md](../SLICES.md#wire-quality-checklist-for-user-facing-transport-adapters) for the telnet adapter:
    1. **Visible vs masked echo.** `read_telnet_line` takes an `EchoMode { Visible, Masked }`; the name and menu prompts use `Visible` (mirrors `amiexpress/express.e:2342`), the password prompt uses `Masked` (mirrors `amiexpress/express.e:1543` — `serPuts('*')` in `getPass2`). The password never appears on the wire in plaintext.
    2. **Line editing.** `<BS>` (`0x08`) and `<DEL>` (`0x7F`) drop the previous byte from the input buffer and emit `<BS><SPACE><BS>` to the client. BS at an empty buffer is a no-op. Mirrors `amiexpress/express.e:1530-1538` and `:2304-2320`.
    3. **CRLF discipline.** All server output is telnet `\r\n`. `<Enter>` from the client triggers a `\r\n` echo so the cursor advances. Disk-loaded `BBSTITLE.txt` is run through the same `\b\n -> \r\n` translation as `Conf02/Menu.txt` so screens authored on the original Amiga render correctly on Linux/macOS/Windows clients.
    4. **Control-byte filtering.** Bytes below `0x20` (other than CR/LF/BS/DEL) are silently dropped from accepted input, matching `lineInput`'s `IF (ch>31)` guard at `amiexpress/express.e:2335`.
    5. **IAC consumed, not stored.** Telnet option negotiation bytes are eaten by the adapter and never reach the input buffer the domain sees. (Already in slice 8; restated here as a requirement of the contract.)
    6. **End-to-end byte assertions in the phase smoke test.** `tests/phase1_smoke.rs` reads the bytes returned to the client between writes and asserts that `sysop` is echoed at the name prompt and that `sysop` (the password) does NOT appear in the bytes between password-submit and the next prompt — a regression guard against accidentally re-introducing plaintext password echo.
  - Source-faithful prompts. The user-visible strings introduced or revised here come from the AmiExpress source verbatim, with a `// amiexpress/<file>:<line>` comment next to each constant:
    - Name prompt: `\r\nEnter your Name: ` (`amiexpress/express.e:31774` for the literal, `:29571` for the `\b\n<prompt> ` wrap).
    - AmiExpress copyright line: `AmiExpress 5 Copyright \u{00A9}2018-2023 Darren Coles\r\n` (`amiexpress/express.e:25690`).
  - NextExpress copyright line printed above the AmiExpress one, derived at compile time from `CARGO_PKG_VERSION`.
  - Tests in `src/adapters/telnet_listener.rs` cover each of the six checklist items independently; the smoke test asserts the end-to-end echo invariant on the running binary.
- **Out of Scope**
  - Mid-line cursor movement and ANSI escape handling for arrow keys / `Ctrl-X` / history (`amiexpress/express.e:2235-2299`) — adds in the toggles / expert-mode slice (Slice 65) where it pulls its weight.
  - `VIEW_PASSWORD` tooltype that reveals real password chars on the local sysop console (`amiexpress/express.e:1544`) — irrelevant until Slice 22 introduces sysop direct logon.
  - Translator / multi-language overrides (`NAME_PROMPT2` tooltype at `amiexpress/express.e:31770`) — sysop tooltype configuration is out of scope for the whole port (per AGENTS.md, "configuration via files rather than a separate program").

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
  - `format_logoff_line`'s byte-tally fields (filled in once Phase 10 lands transfer accounting).

## Slice 13a — Phase 1 wire-and-smoke (composition root + sysop seed)
- **In Scope**
  - Adds `core.allium:config.port` (the TCP port the telnet listener binds on; default `2323`, the AmiExpress-era convention). First read by this slice.
  - `app::main` becomes a real composition root: parses an optional single positional CLI arg as a TOML config path, falls back to `Config::default()` when absent, builds the [`Pbkdf2PasswordHasher`], the [`InMemoryUserRepository`], the [`InMemoryCallerLog`] and the [`TelnetListener`], then `println!`s `Listening on <local_addr>` (a single line, no extra adornment) and calls `listener.run().await`.
  - Config file format: TOML, parsed by `serde` + the `toml` crate. Schema is exactly today's `Config` struct (`port`, `max_nodes`, `bbs_path`, `max_password_failures`); every field is optional and falls back to `Config::default()`. Missing config arg = use defaults; malformed config = panic with a clear "couldn't parse <path>: <error>" message.
  - Seed-data fallback: when the configured `InMemoryUserRepository` ends up empty (which is the only path today, since the slice does **not** introduce a `[[users]]` schema), `app::main` inserts one slot-1 `User` with handle `sysop`, password `sysop` hashed with `Pbkdf210000`, and `access_level = 255`. A `WARNING: seeded default sysop credentials …` line is printed to stderr alongside the `Listening on …` stdout line.
  - Binary smoke test (`tests/phase1_smoke.rs`): spawns the `nextexpress` binary as a subprocess (located via `env!("CARGO_BIN_EXE_nextexpress")`), feeds it a temp TOML with `port = 0`, parses `Listening on <addr>` from stdout, opens a real `TcpStream`, walks the full Phase 1 flow (`Login: sysop` → `Password: sysop` → command `G` → `Goodbye!`) and asserts the connection closes. Kills the child on the way out.
- **Out of Scope**
  - A `[[users]]` array in the config file or any other on-disk user format — deferred to whichever slice eventually replaces `InMemoryUserRepository` with a persistent store. The hardcoded sysop fallback is explicitly a dev seed.
  - `0.0.0.0` / multi-interface bind, IPv6 selection, TLS, SSH transport — Phase 1 binds `127.0.0.1` only.
  - A first-run installer / interactive setup — `nextexpress` with no args is sufficient to telnet against; richer ergonomics arrive when later phases need them.
  - Logging framework — `println!`/`eprintln!` is enough until structured logging is genuinely needed.
