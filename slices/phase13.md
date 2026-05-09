# Phase 13 — User self-service

Toggles, the `W` change-info command, the `S` and `T` stats / time
screens, and the `O` page-sysop chat allowance.

See [SLICES.md](../SLICES.md) for the schema-growth principle, progress
table and asset inventory.

## Slice 65 — Quiet mode + ANSI / RIP / expert toggles
- **In Scope**
  - Adds `Session.ansi_colour`, `Session.quick_logon`, `Session.rip_mode`, `Session.quiet_mode`, `Session.cmd_shortcuts` and `User.expert_mode`.
  - Backfills the boolean-flag `ensures` clauses on `AcceptConnection` (Slice 7 deferred them).
  - Menu commands `M` (ANSI on/off, per `Conf02/Menu.txt`), `X` (expert mode).
- **Out of Scope**
  - RIP rendering itself — the flag is recorded; rendering is out of scope for the BBS core.

## Slice 66 — `W` (change user info) command
- **In Scope**
  - Adds `User.real_name`, `User.internet_name`, `User.preferred_protocol` (first read here).
  - Edit `location`, `phone_number`, `email`, `line_length`, `preferred_protocol`, `flags`.
  - Edit `real_name` / `internet_name` when the current conference's `accepted_name_type` requires it.
- **Out of Scope**
  - Handle changes (sysop-only; not modelled).

## Slice 67 — `S` (user stats) + `T` (time) commands
- **In Scope**
  - Stats screen: `times_called`, `messages_posted`, `bytes_*_total`, `last_call`, `chat_minutes_remaining`.
  - Time screen: `time_remaining`, `bytes_remaining_today`.
- **Out of Scope**
  - Graphs / multi-conference summaries.

## Slice 68 — Sysop chat allowance (`O` page sysop)
- **In Scope**
  - Adds `User.chat_minutes_remaining`, `User.chat_minutes_per_call` (first read here).
  - `chat_minutes_remaining` decrement; `O` command pages the sysop console (no chat protocol yet).
- **Out of Scope**
  - Two-way chat protocol — that's its own subsystem.
