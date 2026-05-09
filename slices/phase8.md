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
