# Tier C — Conference navigation

Conferences and `J` shipped as foundation work; the rules live in
[`specs/conferences.allium`](../specs/conferences.allium) and their
implementation under `rust/src/domain/conference*.rs`. This file finishes
the navigation surface: the prev/next shortcuts, the message-base sibling
commands, and the conference flags editor.

See [SLICES.md](../SLICES.md) for the schema-growth principle and
asset inventory.

## No `CS` command (resolved 2026-06-03)

There is **no `CS` command** in AmiExpress. The legacy dispatch table
(`processInternalCommand`, `express.e:28285`) has no `CS` token, and the
live FS-UAE reference confirmed it. The runtime multi-conference mail
scan is `MS` (`internalCommandMS`, already shipped); the *logon-time*
conference scan is `confScan()` (`express.e:28066`), which is not a menu
command. An earlier roadmap entry proposed a `CS` command with an
invented `Conference <n>:` / `<CR>=next, S=stop` UX — that was dropped as
drift (recorded under Skipped slices in [SLICES.md](../SLICES.md)).

The per-conference scan flags (`ConferenceMembership.mail_scan` and
siblings) that `confScan()` consults are edited by the `CF` command
(Slice C5, below); they gate the conference mail-scan and the `N`
new-files scan (Tier D) — not any `CS` command.

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
  - Walks the conference catalogue looking for the nearest
    neighbour the caller has access to, then calls into
    `Session::explicit_join`. **Note:** `Config`
    (`rust/src/app/config.rs`) has no `num_conf` field; the seam to
    iterate is the existing `&[Conference]` slice from
    `services.conferences()`, not a config integer. Wraps to the
    interactive prompt (slice C2) when no such neighbour exists,
    matching `amiexpress/express.e:24536-24544` / `:24555-24563`.

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

**Status: Done (2026-06-03), landed first in this tier.**
Full design and the live FS-UAE behaviour assessment:
[`docs/superpowers/specs/2026-06-03-cf-conference-flags-design.md`](../docs/superpowers/specs/2026-06-03-cf-conference-flags-design.md).
Decisions: flags live on `ConferenceMembership` (per-conference; every
shipped conference is single-base) and persist through SQLite; `mail_scan`
/ `file_scan` default on (D2); `*` honours the advertised toggle-all the
legacy no-ops (D1); the mask key is read as a line (Enter required), not a
single `readChar` — the wire echo is identical.

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
  - Smoke test: log in, hop via `<` / `>` / `JM`, and edit conference
    flags via `CF`. (`CF` already has its own end-to-end telnet smoke,
    shipped with C5.)
- **Out of Scope**
  - SSH transport for the smoke run (Future).
