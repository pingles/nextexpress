# Phase 6 — Conferences (admin)

Sysop-only: create new conferences, grant or revoke per-user access.

See [SLICES.md](../SLICES.md) for the schema-growth principle, progress
table and asset inventory.

## Slice 35 — Sysop creates conference
- **In Scope**
  - `conferences.allium:SysopCreatesConference` — also creates `MessageBase` number 1 with the given defaults.
- **Out of Scope**
  - Editing existing conferences in place.

## Slice 36 — Sysop grants / revokes access
- **In Scope**
  - `conferences.allium:SysopGrantsConferenceAccess`, `SysopRevokesConferenceAccess`.
  - Idempotent: re-granting is a no-op; revoking sets `granted = false` rather than deleting (preserving counters per `core.allium:ConferenceMembership` "false rows can be kept for history").
- **Out of Scope**
  - Bulk per-area grant by access level (`AREA.NewUser`, `AREA.Normal`, `AREA.Sysop` from `defaultbbs/Access/`).
