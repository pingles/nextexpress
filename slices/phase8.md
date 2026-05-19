# Phase 8 — Messaging (advanced)

Replies, forwards, censored users, attachments, and the destructive
operations: delete, move and edit-header.

See [SLICES.md](../SLICES.md) for the schema-growth principle, progress
table and asset inventory.

## Slice 45 — `ReplyToMail`
- **In Scope**
  - `messaging.allium:ReplyToMail` — `reply_keeps_broadcast` honoured for ALL replies.
- **Out of Scope**
  - Quoting prior message body (presentation concern).

## Slice 46 — `ForwardMail`
- **In Scope**
  - `messaging.allium:ForwardMail` — `forward_header_for` builder, optional note appended.
- **Out of Scope**
  - Forwarding attachments (Slice 48).

## Slice 47 — Censored users + private/private-to-sysop
- **In Scope**
  - Adds `User.censored` field (first read here).
  - `User.censored` forces `private_to_sysop` visibility on `PostMail`.
  - Listing screen shows lower-case glyph for sysop-only mail.
- **Out of Scope**
  - Visibility transitions by sysop (Slice 49).

## Slice 48 — `MailAttachment` + `AttachFileToMail`
- **In Scope**
  - `messaging.allium:MailAttachment` entity referencing a file by name.
  - Both pre-upload and post-upload attachment paths.
- **Out of Scope**
  - The wire transfer for the attached file — that runs through Phase 10 once protocols land.

## Slice 49 — `DeleteMail`, `MoveMail`, `EditMailHeader`
- **In Scope**
  - `messaging.allium:DeleteMail` — soft delete, bumps `lowest_undeleted_message`.
  - `messaging.allium:MoveMail` — atomic delete-then-create across bases, attachments tagged onto the new mail.
  - `messaging.allium:EditMailHeader` — sysop-only subject / addressee rewrite.
- **Out of Scope**
  - Bulk delete / archive.

## Slice 49a — Phase 8 wire-and-smoke (user-facing reply / forward)
- **In Scope**
  - Menu wiring for `RP <num>` (reply) and `FW <num>` (forward)
    top-level commands that drive Slices 45 / 46 end-to-end
    through the compiled binary. The legacy `R` / `F` single-letter
    sub-prompts after reading a message remain a UX improvement
    for a later slice; the two-letter form sidesteps the conflict
    with `R` (read) and `F` (file listing, when it lands).
  - A `phase8_smoke.rs` integration test that spawns the binary
    over telnet, posts an original message, replies to it, and
    forwards it, then reads the resulting mail back to confirm.
- **Out of Scope**
  - Sysop destructive ops (Slice 49b).
  - Quoting prior body / forward attachments (Slice 48 only models
    the metadata; wire transfer is Phase 10).

## Slice 49b — Phase 8 wire-and-smoke (sysop K / Move / EH)
- **In Scope**
  - Sysop menu wiring for the destructive `K` (kill / delete),
    move (cross-base) and edit-header commands that drive Slice
    49's three domain rules end-to-end. Command letters chosen to
    avoid the legacy `FM` (file maintenance) collision.
  - Extension of `phase8_smoke.rs` with a sysop session that
    deletes a mail, moves one across message bases, and rewrites
    the header of a third.
- **Out of Scope**
  - Bulk delete / archive.
