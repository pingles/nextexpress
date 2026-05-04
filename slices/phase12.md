# Phase 12 — Files (admin)

Sysop-controlled file area maintenance: direct uploads, moves, deletes,
admit-from-hold, and the lcfiles / quarantined transitions.

See [SLICES.md](../SLICES.md) for the schema-growth principle, progress
table and asset inventory.

## Slice 58 — `SysopUploadFile`
- **In Scope**
  - `files.allium:SysopUploadFile` — bypass the user upload flow, place `available` immediately.
- **Out of Scope**
  - Bulk import.

## Slice 59 — `MoveFile` + `DeleteFile` + `AdmitHeldFile`
- **In Scope**
  - `files.allium:MoveFile`, `DeleteFile` (soft, status `removed`), `AdmitHeldFile`.
  - Flagged-file rows pointing at a deleted file are dropped per the spec.
- **Out of Scope**
  - Hard purge (audit trail kept per legacy behaviour).

## Slice 60 — `lcfiles` and `quarantined` workflows
- **In Scope**
  - `available -> lcfiles -> available` round-trip; `available -> quarantined -> available` after sysop intervention.
- **Out of Scope**
  - Surfacing low-credit weighting in download ratio computations (covered by Slice 55's ratio function once the formula is parameterised).
