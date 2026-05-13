# Phase 7 — Messaging (write)

Posting mail: single-addressee `E`, broadcast `ALL` / `EALL` addressing,
and the dedicated `C` (comment to sysop) command.

See [SLICES.md](../SLICES.md) for the schema-growth principle, progress
table and asset inventory.

## Slice 42 — `PostMail` rule (single-addressee, `E` command)
- **In Scope**
  - Adds `User.messages_posted` (first read here).
  - `messaging.allium:PostMail` with `broadcast_to = none`.
  - `lookup_user_by_name` and `display_name_of` black-box helpers honouring `Conference.accepted_name_type`.
  - Message-base lock predicate (`lock_msgbase`) implemented as a per-base `Mutex`; treated as a black box per spec.
  - User and per-conference `messages_posted` counters incremented.
- **Out of Scope**
  - Editor itself — line-mode for now; full-screen editor deferred.
  - ALL / EALL (Slice 43).
  - Censored users (Slice 47).

## Slice 43 — Broadcast addressing (ALL / EALL)
- **In Scope**
  - `messaging.allium:BroadcastTo` and `AllowedAddressing` — `addressing_allows` enforced at post time.
  - `AllScanScope` per-conference toggle plumbed into the scan rule.
- **Out of Scope**
  - EALL fan-out across conferences at write time (lazy per spec).

## Slice 44 — `PostCommentToSysop` (`C` command)
- **In Scope**
  - `messaging.allium:PostCommentToSysop` — composes a private message addressed to handle "Sysop", emits `CommentToSysopPosted`.
  - Used by Slice 16's "leave a comment on the way out" exit path.
- **Out of Scope**
  - Out-of-band sysop notification (email, paging — separate adapter).

## Slice 44a — Phase 7 wire-and-smoke
- **In Scope**
  - End-to-end binary smoke test exercising the Phase 7 capabilities through
    the compiled `nextexpress` binary over telnet, verifying the headline
    write flow lands a user-visible feature:
      - posting an `ALL`-addressed message via `E ALL`, with `Recv'd: N/A`
        rendered for the saved mail;
      - rejecting `EALL` on a message base configured with
        `allowed_addressing = individual_or_all`;
      - using the `C` command to leave a comment for the sysop and reading
        it back as a private message addressed to `Sysop`.
  - Confirms the composition root threads the per-msgbase
    `AllowedAddressing` / `AllScanScope` toggles from `conference.toml`
    through to the rule.
- **Out of Scope**
  - EALL fan-out across conferences (Slice 43 keeps this lazy at scan time).
  - Sysop out-of-band notification (Slice 44's Out of Scope still holds).
