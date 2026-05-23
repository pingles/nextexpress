# Tier F — User self-service

`W` (change user info), `B` (bulletins), `GR` (greets). `W` is the
load-bearing command of this tier: it owns most of the user record's
editable surface.

See [SLICES.md](../SLICES.md) for the schema-growth principle and
asset inventory.

## Slice F1 — `W` scaffolding + edit `location`

- **In Scope**
  - Parser: `MenuCommand::ChangeUserInfo`.
  - Drops into the legacy `W` editor screen
    (`amiexpress/express.e:25712-25807`).
  - This slice ships the menu rendering and a *single working
    field* — `location` — chosen because it's the most common edit
    and exercises the full "read field, prompt, validate, write back
    via `UserRepository`" loop end-to-end.
  - All other lines render with the legacy `[DISABLED]` placeholder.
- **Out of Scope**
  - Other fields (slices F2 / F3).
- **Why this shape**: ships visible user value (one editable field
  is enough for an end-user to test the loop) without dragging in
  every field's validation rules at once.

## Slice F2 — `W` extended: `email`, `phone_number`, `line_length`

- **In Scope**
  - Three more field-edit branches with their legacy validators.
  - Adds `User.line_length` (first read here — defaults to `0`
    meaning "auto" per legacy).
- **Out of Scope**
  - Per-conference name-type-driven `real_name` /
    `internet_name` edits (slice F3).

## Slice F3 — `W` extended: name-type + computer / screen / protocol

- **In Scope**
  - Adds `User.real_name`, `User.internet_name`,
    `User.preferred_protocol` (first read here).
  - Edits `real_name` / `internet_name` when the current
    conference's `accepted_name_type` requires it.
  - Computer-type and screen-type lists driven from the same
    `Conf<n>/computertypes` / `screentypes` config files the legacy
    reads.
- **Out of Scope**
  - Handle changes (sysop-only; not modelled).

## Slice F4 — `W` password change

- **In Scope**
  - Adds the legacy password-change branch of `W` (option `6`).
  - Uses the existing `pbkdf2_10000` adapter from Slice 4.
- **Out of Scope**
  - Sysop-side reset (covered by `force_password_reset` in
    `cmds-sysop-console.md`'s `1` slice).

## Slice F5a — `B <n>` (direct bulletin read)

- **In Scope**
  - Parser: `MenuCommand::Bulletin(NumberArg)` for the numeric-arg
    form.
  - Reads `<conf-screen-dir>/Bulletins/Bull<n>.txt` via
    `findSecurityScreen` (legacy
    `amiexpress/express.e:24607-24656`).
- **Why split**: `B 1` is what a user types when the sysop says
  "read bulletin 1." Shipping the direct form ahead of the
  interactive prompt closes the headline use case immediately.

## Slice F5b — `B` (interactive bulletin index)

- **In Scope**
  - No-arg form drops into the
    `Which Bulletin (?)=List, (Enter)=none? ` prompt.
  - `?` branch lists `BullHelp.txt`.
- **Out of Scope**
  - Per-security bulletin variants beyond what `findSecurityScreen`
    already does on disk.

## Slice F6 — `GR` (greets)

- **In Scope**
  - Parser: `MenuCommand::Greets`.
  - Emits the legacy "In memory of those who came before us…"
    decoration block verbatim (`amiexpress/express.e:24411-24421`).
- **Out of Scope**
  - Customisable greets list — legacy is hard-coded; we follow
    suit.

## Slice F-wire — Tier F wire-and-smoke

- **In Scope**
  - Smoke test: log in, run `B` and read a fixture bulletin; `W`
    edits each field once; `GR` emits the banner.
- **Out of Scope**
  - Concurrent-`W`-edit races — single session per user is the
    only contract.
