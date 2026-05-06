# Phase 3 — New user onboarding

The "I've never called this BBS before" path: typing `NEW` at the handle
prompt walks the visitor through registration and lands them at the menu
under a restricted access tier until a sysop validates them.

See [SLICES.md](../SLICES.md) for the schema-growth principle, progress
table and asset inventory.

## Slice 19 — `user_typed_NEW` branch
- **In Scope**
  - Adds the `new_user_registering` state to `Session.state` and the `identifying -> new_user_registering` transition.
  - `session.allium:NameTyped` `user_typed_NEW` branch transitions to `new_user_registering`.
  - Display of the NEWUSERPW screen if available, else built-in prompt.
- **Out of Scope**
  - The actual registration form (Slice 20).

## Slice 20 — `CompleteNewUserRegistration`
- **In Scope**
  - Adds `User.is_new_user`, `User.location`, `User.phone_number`, `User.email`, `User.line_length`, `User.ansi_colour`, `User.account_created`, `User.flags` (first read here).
  - Adds `core.allium:config.default_ratio_mode` and `default_ratio_value` (read by the registration default).
  - Collect handle, location, phone, email, password, line-length, ANSI flag.
  - `session.allium:CompleteNewUserRegistration` rule creates the `User` with the spec's exact defaults and transitions session to `onboarded`.
  - `next_free_slot` black-box function backed by the user repository.
- **Out of Scope**
  - `is_new_user` "awaits sysop validation" gating of access — Slice 21.

## Slice 20a — New-user password gate
- **In Scope**
  - `core/config.allow_new_users` (default `true`), `new_user_password` (default `null`), `max_new_user_password_attempts` (default `3`).
  - `Session.new_user_password_verified`, `Session.new_user_password_attempts`.
  - `session.allium:RejectDisallowedRegistration` — when `allow_new_users = false`, NEW typed bounces straight to `logging_off` with `new_user_rejected`.
  - `session.allium:InitialiseNewUserGate` — sets `verified = (new_user_password = null)` and zeros `attempts` on entering `new_user_registering`.
  - `session.allium:VerifyNewUserPassword` — case-insensitive equality against `new_user_password`, retry budget `max_new_user_password_attempts`, caller-log entry on each failure.
  - `CompleteNewUserRegistration` precondition tightens to require `new_user_password_verified`.
  - Telnet listener: render NEWUSERPW screen (already present) plus the prompt loop with the verbatim AmiExpress text (`Enter New User Password: ` / `Invalid PassWord` / `Excessive Password Failure`); render NONEWUSERS screen on `RejectDisallowedRegistration` with built-in fall-back line.
- **Out of Scope**
  - The per-baud `NONEWATBAUD` variant — modern transports are baud-vestigial.
  - Asset-presence-as-gate convention (legacy AmiExpress took the SCREEN_NONEWUSERS file's existence as the gate; modern config is the source of truth and a later slice can layer the legacy alias on top).

## Slice 21 — Pending-validation gate
- **In Scope**
  - When `User.is_new_user`, restrict `access_level` interpretation to a "newuser" tier (no posting, no downloads).
  - `has_access(user, right)` black-box function plumbed; `read_message` and `comment_to_sysop` granted, others denied.
- **Out of Scope**
  - The sysop "validate user" command (lands with the rest of Phase 6 sysop-conference-admin work).
