# Phase 7 — Messaging (read)

Mail entity and on-disk store, read pointers, the `R`, `M` and `N`
commands, and auto mail scan on join.

See [SLICES.md](../SLICES.md) for the schema-growth principle, progress
table and asset inventory.

## Slice 37 — `Mail` entity + on-disk message store
- **In Scope**
  - `messaging.allium:Mail` entity with header + body, `transitions visibility`, `FromNameMatchesAuthor` and `DeletedMessagesHaveNoActiveReceived` invariants.
  - File-based store (one file per message, header file separate per legacy convention).
  - `MessageNumbersUniquePerBase` and `HighestMessageMatchesMaxNumber` invariants enforced by the store.
- **Out of Scope**
  - `MailAttachment` (Slice 48).
  - External message bases / `ext_msg_num` (deferred).

## Slice 38 — `ReadPointers` entity
- **In Scope**
  - `core.allium:ReadPointers` with `last_read`, `last_scanned`, `new_since`.
  - `ReadDoesNotExceedScanned` invariant.
  - `read_pointers_for(user, msgbase)` black-box helper.
- **Out of Scope**
  - Pointer reset commands.

## Slice 39 — `ReadMail` rule + `R` menu command
- **In Scope**
  - `messaging.allium:ReadMail` — gate on `has_access(user, read_message)` and `can_read(user, mail)`.
  - Sets `received_at` for the addressee on first read; advances `last_read`.
  - `DeletedMessagesNotAddressableByLastRead` invariant covered.
- **Out of Scope**
  - Reply / forward / delete (Phase 9).

## Slice 40 — `ScanMail` + `M`/`N` menu commands
- **In Scope**
  - `messaging.allium:ScanMail` — emits `MailScanCompleted`, advances `last_scanned`.
  - `count_unread_for`, `first_unread_number_for` black-box helpers.
- **Out of Scope**
  - EALL fan-out (`messaging.allium` open question — lazy at scan time).

## Slice 41 — Auto mail scan on join
- **In Scope**
  - `conferences.allium:ScanMailOnJoin` — `follow_pointer` and `force_all` modes wire into the scan rule.
  - Display `SCREEN_MAILSCAN` when there are unread messages.
- **Out of Scope**
  - Cross-conference (zoom) scans.
