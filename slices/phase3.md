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

## Slice 21 — Pending-validation gate
- **In Scope**
  - When `User.is_new_user`, restrict `access_level` interpretation to a "newuser" tier (no posting, no downloads).
  - `has_access(user, right)` black-box function plumbed; `read_message` and `comment_to_sysop` granted, others denied.
- **Out of Scope**
  - The sysop "validate user" command (lands with the rest of Phase 6 sysop-conference-admin work).
