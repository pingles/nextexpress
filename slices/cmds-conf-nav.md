# Tier C — Conference navigation

Conferences and `J` shipped as foundation work; the rules live in
[`specs/conferences.allium`](../specs/conferences.allium) and their
implementation under `rust/src/domain/conference*.rs`. This file finishes
the navigation surface: the legacy "conference scan" command-line
form, the prev/next shortcuts, the message-base sibling commands,
and the conference flags editor.

See [SLICES.md](../SLICES.md) for the schema-growth principle and
asset inventory.

## Slice C1 — `CS` (surface the existing `ConferenceScan` rule)

- **In Scope**
  - Parser: `MenuCommand::ConferenceScan`.
  - Dispatches to the already-shipped `Session::start_conference_scan`
    / `step_conference_scan` (Slice 33).
  - Per-step wire text mirrors `joinConf`'s scan-display path: the
    `Conference <n>: <name>` line, then the mail-scan summary
    inherited from Slice 41.
  - Sub-prompt at end of each conference offers
    `<CR>=next, S=stop` per the legacy scan UX.
- **Out of Scope**
  - Filtering the scan by `ConferenceMembership.scan_flags`
    (covered by Slice C5's `CF` command).
- **Why it's first in this tier**: the domain rule is already
  implemented; this slice is a parser/dispatch wiring exercise and
  delivers visible value immediately.

## Slice C2 — `J` no-arg interactive prompt (parity fix)

- **In Scope**
  - When `J` is typed with no argument, NextExpress today rejects
    with `Usage: J <conference-number>`. Legacy
    (`amiexpress/express.e:25143-25151`) displays SCREEN_JOINCONF
    and prompts `Conference Number (1-N): ` via `lineInput`; blank
    input returns silently to the menu.
  - This slice replaces the rejection with the interactive prompt.
- **Out of Scope**
  - The `JM` message-base sub-prompt (Slice C4).

## Slice C3 — `<` / `>` (prev / next accessible conference)

- **In Scope**
  - Parser: `MenuCommand::PrevConference` /
    `MenuCommand::NextConference`.
  - Walks the `Config.num_conf` integer space looking for the
    nearest neighbour the caller has access to, then calls into
    `Session::explicit_join`. Wraps to the interactive prompt
    (slice C2) when no such neighbour exists, matching
    `amiexpress/express.e:24536-24544` / `:24555-24563`.

## Slice C4a — `JM <n>` (explicit join message base)

- **In Scope**
  - `MenuCommand::JoinMsgBase(NumberArg)` for the numeric-arg form
    mirrors `internalCommandJM` (`amiexpress/express.e:25185`). A
    `.`-dotted arg (`JM 2.3`) delegates to `J` per the legacy.
- **Out of Scope**
  - No-arg interactive prompt (lands in C4b with the sibling
    shortcuts since it reuses the same `lineInput` block).
  - `<<` / `>>` sibling navigation (slice C4b).
- **Why split**: shape is identical to the already-shipped explicit
  `J <n>` — one TDD turn ships visible value, decoupled from the
  accessible-neighbour walk in C4b.

## Slice C4b — `<<` / `>>` and `JM` interactive prompt

- **In Scope**
  - `<<` / `>>` step through the current conference's message-bases,
    same accessible-only walk as `<`/`>` (legacy:
    `:24566-24592`).
  - `JM` no-arg form drops into the same interactive prompt the
    legacy renders (`:25197-25208`).
- **Out of Scope**
  - Per-msgbase access lists distinct from per-conference (legacy
    does not split).

## Slice C5 — `CF` (conference flags editor)

- **In Scope**
  - Adds `ConferenceMembership.mail_scan`,
    `ConferenceMembership.mailscan_all`,
    `ConferenceMembership.file_scan` and
    `ConferenceMembership.zoom_scan` (first read here).
  - Renders the legacy two-column listing
    (`amiexpress/express.e:24691-24747`) with the `M / A / F / Z`
    columns.
  - Edit loop accepts `M` / `A` / `F` / `Z` to pick which mask, then
    a conference-numbers expression
    (`<digits,> | + | - | *`) to toggle/set/clear.
- **Out of Scope**
  - Forced-newscan / no-newscan tooltype overrides — those land
    with the per-conference `Conf.toml` config schema, not here.

## Slice C-wire — Tier C wire-and-smoke

- **In Scope**
  - Smoke test: log in, run `CS` and walk the scan, hop via `<` /
    `>` / `JM`, edit conference flags via `CF` and confirm the new
    mask is honoured on the next scan.
- **Out of Scope**
  - SSH transport for the smoke run (Future).
