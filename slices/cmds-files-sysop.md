# Tier D (sysop) — File area maintenance

Sysop-only file admin: bypass uploads, move / delete / admit-from-hold,
and the lcfiles / quarantined transitions. The browse and transfer
tiers ([cmds-files-list.md](cmds-files-list.md),
[cmds-files-transfer.md](cmds-files-transfer.md)) must land first
since these slices reuse their entities and rules.

See [SLICES.md](../SLICES.md) for the schema-growth principle and
asset inventory.

## Slice D-S1 — `US` (sysop upload) and `SysopUploadFile`

- **In Scope**
  - Parser: `MenuCommand::SysopUpload`.
  - `files.allium:SysopUploadFile` — bypass the user upload flow,
    place `available` immediately (no playpen / no background
    check).
  - Mirrors `internalCommandUS` (`amiexpress/express.e:25660`).
- **Out of Scope**
  - Bulk import.

## Slice D-S2 — `FM` (file maintenance: move, delete, admit)

- **In Scope**
  - Parser: `MenuCommand::FileMaintenance(args)`.
  - `files.allium:MoveFile`, `DeleteFile` (soft, status `removed`),
    `AdmitHeldFile` — driven from the legacy `internalCommandFM`
    UI (`amiexpress/express.e:24889`).
  - Flagged-file rows pointing at a deleted file are dropped per the
    spec.
- **Out of Scope**
  - Hard purge (audit trail kept per legacy behaviour).

## Slice D-S3 — `lcfiles` and `quarantined` workflows

- **In Scope**
  - `available -> lcfiles -> available` round-trip.
  - `available -> quarantined -> available` after sysop intervention.
- **Out of Scope**
  - Surfacing low-credit weighting in download ratio computations —
    that's covered by [cmds-accounting.md](cmds-accounting.md)'s
    ratio refinement once the formula is parameterised.

## Slice D-S-wire — Tier D (sysop) wire-and-smoke

- **In Scope**
  - As sysop, US-upload a fixture file, FM-move it, FM-delete it,
    confirm the listing reflects each transition.
- **Out of Scope**
  - Concurrent-sysop edits — the lock is per area and is already
    asserted in the browse tier's tests.
