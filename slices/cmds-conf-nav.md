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

**Status: Done (2026-06-10), pinned against the live AmiExpress 5.6.0
reference (`comparison/evidence-tierC/live-observations.md`).**
Decisions: the prompt is single-shot — blank aborts with one CRLF,
anything else takes the legacy `Val` of the line (optional `-` sign +
digit prefix; `+` is not a sign) clamped into `[1,N]`, N = the highest
conference number; direct `J <arg>` values are *not* clamped — bare,
zero, negative, non-numeric and out-of-range arguments all open the
prompt (`express.e:25142` vs `:25153-25154`). Denied joins (parity
fix to shipped `J <n>`) print the legacy no-access notice and stay in
the current conference — no first-accessible fallback, no disconnect
(`express.e:25156-25158`). The built-in `JoinConf` fallback screen is
now empty: the reference renders nothing before the prompt when the
asset is absent. Eof / idle timeout at the prompt returns silently to
the menu loop (the CF precedent; legacy would take the
`RESULT_TIMEOUT` bell-and-grace path). **Interim until C4a**: the
dotted (`J 1.1`) and two-token (`J 1 2`) message-base forms route
into the conference prompt rather than joining silently (the live
reference joins `J 1.1` directly and gives `J 1 2` the message-base
prompt) — `TODO(C4a)` markers sit in `menu_command.rs` / `join.rs`.

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

**Status: Done (2026-06-10), pinned against the live AmiExpress 5.6.0
reference (`comparison/evidence-tierC/live-observations.md`).**
Decisions: the walk follows the sorted conference catalogue (so
non-contiguous numbering works), skipping inaccessible conferences
silently (`express.e:24536-24538` / `:24555-24557`); a hit joins
through the same `handle_explicit_join` machinery as a direct `J <n>`
— the wire output is byte-identical (`joinConf(newConf,1,FALSE,FALSE)`,
`:24543` / `:24562`); past either edge the command runs the C2
interactive prompt (`internalCommandJ('')`, `:24541` / `:24560`) — no
wraparound. The parser dispatches on the head token alone, so trailing
parameters are discarded (`< 2` is `<`) while `<<` / `>>` / `<2` stay
unknown until C4b (exact `StrCmp` dispatch, `express.e:28322-28329`).
**Deliberate deviation**: the legacy `ACS_JOIN_CONFERENCE` gate
(`:24531` / `:24550`) is not ported — the port has no join right yet
and `J` does not gate today, so `<` / `>` stay consistent with it
(rationale in the handler doc comments, `menu_flow/join.rs`). The
missing post-join mail-stats block (`express.e:5092-5109`) is the
already-known `J` divergence, shared automatically; the eventual stats
slice extends `<` / `>` for free via the shared join machinery.

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

**Status: Done (2026-06-10), pinned against the live AmiExpress 5.6.0
reference (`comparison/evidence-tierC/live-observations.md`) and the
raw source (`comparison/evidence-tierC/legacy-JM.md`).**
Decisions: `MenuCommand::JoinMsgBase(MsgBaseArg)` carries the `Val` of
the first token only (extra tokens ignored, `express.e:25199-25208`);
a `.`-dotted first token delegates the raw params to the `J` logic at
parse time (`express.e:25203-25205` — observed: `JM 1.1` ≡ `J 1.1`).
The dotted / two-token `J` forms now join the requested base
(replacing C2's interim conference-prompt routing): `Val` of the
first token is the conference (stopping at the `.`), the text after
the first `.` — else the second token — is the base (`J 1 2 3` =
conf 1 base 2); the base request survives the conference prompt, and
the access check on the resolved conference precedes the base range
check (`express.e:25156` vs `:25168`). Default base when unspecified:
the conference's primary base. The domain explicit-join transition
takes the requested base and defensively resets a base the conference
does not hold to the PRIMARY base (the legacy `joinConf` clamp,
`express.e:4995`) — range checks that decide between joining and
prompting stay in the handlers, as in the legacy split. Multi-base
join announcements append ` [<base>]` (`express.e:5077-5084`);
`JM <current>` re-joins in full (no "already there" check); read
pointers stay per-msgbase (`ConferenceMembership.pointers`).

**Single-base gate**: when the current conference has exactly one
message base, every non-dotted `JM` form (no-arg, `JM 1`, `JM 9`,
`JM abc`) writes exactly
`\r\nThis conference does not contain multiple message bases\r\n\r\n`
and stays — no join, no prompt (`express.e:25211-25215`). NextExpress
equates the legacy "`NMSGBASES` tooltype absent" with
`bases.len() == 1`; the legacy nuance of an explicitly-set
`NMSGBASES=1` producing a `(1-1)` prompt instead is **deliberately
not modelled** (NextExpress has no per-conference tooltype layer; the
base count in `conference.toml` is the single source of truth).

**Interim until C4b** (closed when C4b landed): `JM` with a
missing/out-of-range argument on a multi-base conference, and `J`
with an explicit out-of-range base (`J 1 2` on a single-base
conference — the live reference prompts even there), returned to the
menu silently until C4b shipped the
`Message Base Number (1-N): ` prompt (`express.e:25169-25180` /
`:25220-25230`).

- **In Scope**
  - `MenuCommand::JoinMsgBase(MsgBaseArg)` for the numeric-arg form
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

**Status: Done (2026-06-10), pinned against the live AmiExpress 5.6.0
reference (`comparison/evidence-tierC/live-observations.md`) and the
raw source (`comparison/evidence-tierC/legacy-prevnext.md` /
`legacy-JM.md`).**
Decisions: `<<` / `>>` dispatch on the exact head token (trailing
parameters discarded, `<<<` unknown — `StrCmp`,
`express.e:28324-28329`) and step `currentMsgBase ∓ 1`
(`internalCommandLT2`/`GT2`, `express.e:24566-24592`): in bounds the
step is a full message-base join byte-identical to `JM <n>`; past
either edge it runs the `JM` no-arg flow — the single-base notice on
a single-base conference (observed live for both commands), the
interactive prompt on a multi-base one. No wraparound, and no
security gate on the direct path (the legacy has none there). The
`Message Base Number (1-N): ` prompt is single-shot: the
`JoinMsgBase` screen precedes it when installed (conference-local
`Conf<NN>/JoinMsgBase.txt` wins over `Screens/JoinMsgBase.txt`,
`express.e:25221-25222`; empty built-in fallback for parity), and it
resolves against the conference the caller is *currently* in — the
legacy `confScreenDir` is repointed only inside `joinConf`
(`:5052`) — while the `(1-N)` bound is the TARGET conference's base
count (`:25167`). Blank input aborts with one CRLF; Eof / idle
returns silently. **Clamp asymmetry, pinned by tests**: `JM`'s
prompt answer is clamped into `[1,N]` (`express.e:25233-25234`);
`J`'s message-base prompt answer is passed to the join *unclamped*
(`:25179`), where the domain resets a base the conference does not
hold to the primary (`:4995`) — so `9` at a `(1-2)` prompt joins
base 2 via `JM` but base 1 via `J`.

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
