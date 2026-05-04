# Phase 10 — Files (browse and flag)

File areas as data, the flagged-files list, and the `A` / `Z` commands
for editing flags and searching.

See [SLICES.md](../SLICES.md) for the schema-growth principle, progress
table and asset inventory.

## Slice 50 — `Bytes` value type + `FileArea` + `File` entities
- **In Scope**
  - Introduces the `Bytes` value type (`core.allium:Bytes`) — `count: u64`, ordering, addition, saturating subtraction.
  - `core.allium:FileArea`, `files.allium:File` (using `Bytes` for `File.size`) with `transitions status`.
  - File listing reads `Conf<n>/Dir<m>` (legacy layout) and surfaces filename, size, description, status.
- **Out of Scope**
  - BCD packing (`core.allium:Bytes` notes BCD is a storage decision — adapter concern).
  - Sysop-uploaded files (Slice 58).

## Slice 51 — `FlagFile` / `UnflagFile`
- **In Scope**
  - `files.allium:FlagFile` and `UnflagFile` rules; per-session flagged list bounded by `max_flagged_files()` (legacy `MAX_FLAGGED_FILES = 1000`).
  - `FlaggedFilesAreDownloadable` invariant.
- **Out of Scope**
  - Persisting the flagged list across sessions (`files.allium` open question).

## Slice 52 — `A` (edit file flags) + `Z` (zippy search) commands
- **In Scope**
  - List + edit the flagged set; zippy search across one or more areas filtered by substring.
- **Out of Scope**
  - Wildcard / regex search syntax; substring is enough for the slice.
