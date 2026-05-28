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
  still carries only `unread_count` / `first_unread_number` /
  `highest_message` and **no listing field**, and `walk()` counts the
  matching rows then discards them. The app renders only
  `render_scan_summary` (`wire_text.rs`, the `You have N new
  messages…` one-liner) — the `Type  From  Subject  Msg` table header
  literal does not exist yet.
- **In Scope**
  - Extend the domain `ScanResult` with the `listing: Vec<MailScanRow>`
    the spec already models, and have `walk()` collect the rows it
    currently throws away.
  - Wire format mirrors the legacy table header
    `[32mType  From  Subject  Msg[0m` plus one row per unread mail
    (legacy `searchNewMail`, `amiexpress/express.e:11713-11739`).
- **Out of Scope**
  - Compact / wide column variants — legacy has one column layout.

## Slice B4 — `R` sub-prompt scaffolding

- **In Scope**
  - Once `R <num>` (or `R` no-arg → "first unread") has printed a
    message via the existing `ReadMail` rule, the session enters the
    sub-prompt loop modelled on `amiexpress/express.e:11972`
    (`readMSG`).
  - Wire text: `Msg. Options: A,D,M,F,R,L,Q,?,??,<CR> (N>M):`
    with the same ANSI as the legacy.
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
  - Cross-conference forwarding — that's an open question in
    `messaging.allium`.

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
