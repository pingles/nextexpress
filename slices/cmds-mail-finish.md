# Tier B — Mail UI completion

The mail subsystem (read / scan / post / comment / reply / forward /
kill / move / edit-header) is wired in the foundation (Phases 6–8 in
the original plan; the rules are now in
[`specs/messaging.allium`](../specs/messaging.allium) and the
implementation under `rust/src/domain/messaging/`). The slices here
finish the parity
work: surface the multi-conference scan command, re-shape the M/N
keys to match legacy semantics, and bring the legacy `R` sub-prompt —
the BBS's primary mail-reading UI — to NextExpress so the top-level
`RP` / `FW` / `K` / `MV` / `EH` shortcuts can be retired.

See [SLICES.md](../SLICES.md) and [COMMAND_PARITY.md](../COMMAND_PARITY.md)
for the cross-reference of every drift NextExpress currently carries.

## Slice B1 — `MS` (multi-conference mail scan, was Slice 49d)

**Note:** `MS` already exists as a token — Tier A's A8 moved the
single-conference scan-all there (`MS` → `MenuCommand::Scan(ScanArg::All)`)
when it rebound bare `M` to the ANSI toggle. This slice *upgrades* that
binding to the legacy multi-conference scan.

- **In Scope**
  - New rule `messaging.allium:ScanAllMail` — for every conference
    the caller has access to, in numeric order, runs the existing
    `ScanMail` rule with `FORCE_MAILSCAN_ALL` semantics.
  - Re-point `MS` from `Scan(ScanArg::All)` to the new
    `MenuCommand::ScanAllMail`.
  - Wire text: header `Scanning conferences for mail...`
    (`amiexpress/express.e:25258`), per-conference banner, then the
    listing row block from Slice B3 below.
- **Out of Scope**
  - Cross-conference de-duplication — the legacy lists each
    conference's hits independently.

## Slice B2 — `N` (mail) semantic fix and new-mail listing

- **In Scope**
  - Re-binds the parser: `MenuCommand::Scan(ScanArg::New)` (the
    current `N` binding) moves to a no-op pending Tier D's
    `cmds-files-list.md`'s `N` (new files scan).
  - The existing auto-mail-scan path on join (Slice 41) is unchanged
    — that's where users see new mail today.
  - Documents the divergence: legacy `M` and `N` do *not* run mail
    scans — they're ANSI-toggle and new-files-scan respectively. Mail
    scan in legacy is reached via auto-mail-scan-on-join, the `R`
    sub-prompt's `L`ist option, or the explicit `MS` command.
- **Depends on**: Tier A's `M` (ANSI toggle) slice. Lands as a pair.
- **Why split out**: the rebinding touches one parser case and one
  dispatch arm; isolating it from the listing-row work keeps each
  slice reviewable in one sitting.

## Slice B3 — `ScanMail` listing rows (was Slice 49c)

- **Spec is already ahead of the code.** The `MailScanRow` value
  (`messaging.allium:161`) and the `listing:` field on
  `MailScanCompleted` (`messaging.allium:431`) were added in commit
  `06ea1cd`. This slice is therefore a *code* slice, not a spec one:
  the Rust domain `ScanResult` (`rust/src/domain/messaging/scan_mail.rs`)
  still carries only `from` / `unread_count` / `first_unread_number` /
  `highest_message` and **no listing field**, and `walk()` counts the
  matching rows then discards them. The app renders only
  `render_scan_summary` (`wire_text.rs`, the `You have N new
  messages…` one-liner) — the column-padded
  `Type … From … Subject … Msg` table header (see In Scope below) does
  not exist yet.
- **In Scope**
  - Extend the domain `ScanResult` with the `listing: Vec<MailScanRow>`
    the spec already models, and have `walk()` collect the rows it
    currently throws away.
  - Wire format mirrors the legacy `searchNewMail`
    (`amiexpress/express.e:11651`), `currentConf=0` branch. The header
    is **three** writes (`:11713-11715`), not one literal: a green
    fixed-width column line (`\x1b[32mType … From … Subject … Msg`,
    `\b\n`), a yellow dashes separator (`\x1b[33m------- … -------`,
    `\b\n`), then a standalone `\x1b[0m` (no `\b\n`). Each unread mail
    is one row in the format
    `'\s  \l\s[29]  \l\s[21]  \x1b[0m\z\r\d[6]\b\n'` (`:11720`): status
    `\s` (7-char `Public `/`Private`) + 2 spaces, From left-justified to
    29 + 2 spaces, Subject left-justified to 21 + 2 spaces, `\x1b[0m`,
    then Msg right-justified zero-padded to 6.
  - The table appears only in the multi-conf `MS` (`currentConf=0`)
    path; the single-conf scan-on-join (`currentConf<>0`) prints no
    table — just `\b\nFound Mail!` (`:11737`). Both end with
    `\b\nWould you like to read it now ` + `yesNo(1)` (`:11739`).
- **Out of Scope**
  - Compact / wide column variants — legacy has one column layout.

## Slice B4 — `R` sub-prompt scaffolding

- **In Scope**
  - Once `R <num>` (or `R` no-arg → "first unread") has printed a
    message via the existing `ReadMail` rule, the session enters the
    sub-prompt loop modelled on `amiexpress/express.e:11972`
    (`readMSG`).
  - Wire text: the ANSI-coloured prompt is assembled piecewise at
    `amiexpress/express.e:12016-12021`, not a single literal. The
    always-present skeleton is
    `Msg. Options: A,F,R,L,Q,?,??,<CR> ( <range> )>: ` where `<range>`
    is the runtime message-range string (e.g. `5+10`, built at
    `:12010`). `D` (delete) is inserted after `A` only for callers with
    `ACS_DELETE_MESSAGE` (`:12017`) and `M` (move) only for
    `ACS_SYSOP_READ` (`:12018`), so a fully-privileged caller sees
    `Msg. Options: A,D,M,F,R,L,Q,?,??,<CR> ( <range> )>: `. Carry the
    legacy ANSI escapes verbatim. (There is no `(N>M):` literal.)
  - `<CR>` advances to the next message in the current msgbase;
    `Q` returns to the menu prompt.
- **Out of Scope**
  - Options other than `<CR>` / `Q` — added in B5.
- **Why split**: ship the legacy primary mail-reading loop with the
  smallest possible surface first; users get the legacy *feel*
  immediately and the remaining options accrete behind it.

## Slice B5 — `R` sub-prompt full options

- **In Scope**
  - `A`gain (re-display current), `R`eply (drops into B6 below),
    `F`orward, `D`elete (sysop), `M`ove (sysop), `L`ist (calls into
    B3's listing), `?` short help, `??` long help.
  - Existing top-level `RP`, `FW`, `K`, `MV`, `EH` parsers stay for
    one release as deprecated shortcuts; the smoke test asserts both
    paths reach the same domain rule.
- **Out of Scope**
  - Retiring the top-level shortcut commands — that's a later slice
    once the sub-prompt has shipped and the deprecation notice has
    been in place for a release.

## Slice B6 — Sub-prompt `R`eply / `F`orward refinement

- **In Scope**
  - Reply / Forward inside the sub-prompt are the same code path as
    the existing top-level `RP` / `FW` (Slices 45 / 46), but the
    subject defaulting (`Re: <original>`), the recipient inference
    (`From` of the original) and the abort behaviour
    (no-notice on blank) are aligned with `readMSG` rather than the
    NextExpress shortcut wire text.
- **Out of Scope**
  - Cross-conference forwarding — `messaging.allium`'s `ForwardMail`
    rule (lines 301–321) only models forwarding within the source
    base's conference; a cross-conference variant is unspecified. (It
    is *not* one of the spec's open questions — the only
    "across conferences" question, line 573, concerns EALL routing.)

## Slice B7 — `E` and `C` wire-text drift fixes

- **In Scope**
  - Item-by-item application of the drift list in
    `COMMAND_PARITY.md` §1 rows 13–21 — restoring legacy ANSI on the
    `To` / `Subject` / `Private` prompts, flipping the default for
    Private to **Y**, adopting the verbatim "User does not exist!!"
    text, and so on.
- **Out of Scope**
  - The legacy screen-mode editor (rows 20 / `edit()`) — keeping the
    NextExpress line-mode editor is a deliberate divergence (see
    `COMMAND_PARITY.md` row 20).

## Slice B-wire — Tier B wire-and-smoke

- **In Scope**
  - Smoke test exercises: log in, run `MS`, drop into a message via
    `R 1`, navigate forward with `<CR>`, reply via the sub-prompt,
    return to the menu.
  - Assert the verbatim sub-prompt wire bytes and that the
    deprecated top-level shortcuts still work.
- **Out of Scope**
  - Removing the top-level shortcuts (slice B8 below).

## Slice B8 — Retire top-level `RP` / `FW` / `K` / `MV` / `EH` shortcuts

- **In Scope**
  - Drop the `MenuCommand::Reply` / `Forward` / `Kill` / `Move` /
    `EditHeader` arms from `menu_command.rs` and `menu_flow/mod.rs`
    once the sub-prompt has been the released default for one
    release cycle.
  - Existing tests covering the top-level forms move to assert the
    R sub-prompt equivalent.
  - Add a one-line entry to `COMMAND_PARITY.md` noting the removal
    so future readers see why typing `RP 1` now errors as unknown.
- **Out of Scope**
  - Re-introducing them under a feature flag — by the time this
    slice ships, the sub-prompt is the only legitimate path.
