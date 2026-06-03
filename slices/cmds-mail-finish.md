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

## This round (2026-05-30): B4 + B5 + B6 + B8 — **Done**

The full `R` sub-prompt shipped and the five non-legacy top-level
shortcuts were retired, in this commit sequence on `tier-b-read-subprompt`:

- **B4** scaffolding (`<CR>`/`Q`); **B5a** dispatch + `A`gain; **B5b**
  `R`eply / `F`orward; **B5c** `D`/`M`/`EH` (gated); **B5d** skeleton
  `D`/`M` insertion + `?`/`??` help + `L`ist with `checkForPause`
  pagination.
- **B6** sub-prompt reply / forward abort silently.
- **B8** retired `RP`/`FW`/`K`/`MV`/`EH` (now unknown commands); the
  `phase8` binary smokes drive the operations through the sub-prompt. A
  latent stale-`highest` bug (reply/forward post mid-loop) was fixed:
  the range bound is now re-read live each turn.

Still open: **B7** (`E` / `C` wire-text drift, independent), **B9** (the
`ACS_*` access-flag fidelity), the no-arg `R` interactive entry
(**B10** below), and the deeper `forwardMSG` To-header / body-copy parity
(COMMAND_PARITY row 23).

The original framing of the round follows.

By default B8 (retiring the top-level `RP` / `FW` / `K` / `MV` / `EH`
shortcuts) waited "one release cycle" after the sub-prompt shipped. For
this round that deprecation window is **deliberately collapsed** at the
sysop's direction: we land the full `R` sub-prompt (B4 scaffolding +
B5 options) and then retire the five non-legacy top-level commands in
the same body of work, because those five never existed as legacy menu
commands — they are sub-prompt options the NextExpress port surfaced
early as a stop-gap. The deletion is therefore safe to do *as soon as*
the sub-prompt provides the equivalent options (B5), with no capability
gap. Implementation order this round is **B4 → B5 → B6 → B8**, with the
**B-wire** closing smoke running *after* B8 (so it asserts the retired
shortcuts now error, rather than that they still work). **B7** (the
`E` / `C` wire-text drift fixes) is independent of the sub-prompt and
can land separately.

**Design decisions for this round:**

- **The sub-prompt loop is an application / `menu_flow` concern**, not a
  new domain rule — the same call the project made for the `MS` listing
  table (a "code slice, not a spec one"). The existing `ReadMail` domain
  rule (`rust/src/domain/messaging/read_mail.rs`) is reused *per message*;
  the loop wraps it. It lives in a new focused module
  `rust/src/app/menu_flow/read_subprompt.rs`, driven from
  `handle_read_mail`'s `ReadMailOutcome::Read` arm
  (`rust/src/app/menu_flow/read_mail.rs`) — the loop needs the live
  terminal and `self.services` to fetch the next message, which the
  terminal-free use case in `app/menu/read_mail.rs` cannot host.
- **A lightweight `messaging.allium` surface anchors the sub-prompt** —
  its option set and the `D` / `M` / `EH` access gates map to existing
  rules. The ANSI wire bytes stay in code (a surface concern the spec
  excludes); the *option-to-rule* mapping and the gating are modelled in
  the spec, authored before the code so the primary mail UI has a spec
  anchor and B8's deletion has a spec justification (the top-level
  shortcuts were never in the spec; the sub-prompt options are).

## Slice B1 — `MS` (multi-conference mail scan, was Slice 49d) — **Done**

Shipped together with **B3** (the listing table): `MS` now binds to a
new `MenuCommand::ScanAllMail` and walks every accessible conference.
The orphaned `ScanArg::All` variant was removed (nothing parsed to it
once `MS` moved off it); `N` still maps to `Scan(ScanArg::New)` pending
B2. The walk lives in the terminal-free `app::menu::scan_all_mail` use
case (classifying each base as `Listing` / `NothingNew` / `NoMatch` /
`NoStore` / `Error`), rendered by `app::menu_flow::scan_all_mail`.

**Deferred to B4/B5:** the `\b\nWould you like to read it now `
+ `yesNo(1)` prompt and the drop-into-read it triggers
(`amiexpress/express.e:11738-11745`). It is interactive-reading
surface, not part of the spec's `ScanAllMail` rule, and overlaps the
`R` sub-prompt slices; the existing scan output never had it either,
so deferring is no regression.

**Note:** `MS` already existed as a token — Tier A's A8 moved the
single-conference scan-all there (`MS` → `MenuCommand::Scan(ScanArg::All)`)
when it rebound bare `M` to the ANSI toggle. This slice *upgraded* that
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

## Slice B2 — `N` (mail) semantic fix — **Done**

`N`'s mail-scan binding (a NextExpress drift — legacy `N` is the
new-files scan, `express.e:25275`) is removed. Rather than a silent
no-op, `N` is now an **unknown command** (it falls through to
`MenuCommand::Unknown` and emits the standard unknown-command notice),
and its line is dropped from `Conf02/Menu5.txt`. `N` and its menu entry
return in Tier D (`cmds-files-list.md`) as the real new-files scan.

- **What was removed:** since `N` was the only consumer of
  `MenuCommand::Scan(ScanArg)`, the whole `Scan` variant, the `ScanArg`
  enum, `parse_scan_command`, the dispatch arm, the terminal handler
  `handle_scan_mail` and its file `menu_flow/scan_mail.rs` are all gone.
- **What stays:** the terminal-free `app::menu::scan_mail` use case —
  the auto-mail-scan-on-join path (Slice 41) is its only remaining
  caller, so it is *not* dead and was kept.
- **The divergence, for the record:** legacy `M` and `N` do *not* run
  mail scans — they're the ANSI toggle and the new-files scan. Mail
  scan in the legacy is reached via auto-mail-scan-on-join, the `R`
  sub-prompt's `L`ist option, or the explicit `MS` command.

## Slice B3 — `ScanMail` listing rows (was Slice 49c) — **Done (with B1)**

The domain `ScanResult` now carries `listing: Vec<MailScanRow>` (the
spec's value, `messaging.allium:MailScanRow`), `walk()` collects the
rows it used to discard, and `app::wire_text::render_scan_listing_table`
renders the three-write header + per-row format below. The table is
emitted only on the multi-conference `MS` path; the single-conference
scan-on-join still renders just its summary line. The
`Would you like to read it now` tail is deferred (see B1 above).

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
  - Once `R <num>` has printed a message via the existing `ReadMail`
    rule, the session enters the sub-prompt loop modelled on
    `amiexpress/express.e:11972` (`readMSG`). The loop lives in a new
    `rust/src/app/menu_flow/read_subprompt.rs`, called from the
    `ReadMailOutcome::Read` arm of `handle_read_mail`.
  - Wire text: the ANSI-coloured prompt is assembled piecewise at
    `amiexpress/express.e:12016-12021`, not a single literal. The
    always-present skeleton is
    `Msg. Options: A,F,R,L,Q,?,??,<CR> ( <range> )>: ` where `<range>`
    is the runtime message-range string (e.g. `5+10`). `D` (delete) is
    inserted after `A` only for callers with `ACS_DELETE_MESSAGE`
    (`:12017`) and `M` (move) only for `ACS_SYSOP_READ` (`:12018`). B4
    emits the **ungated** skeleton (no `D` / `M`); the gates land in B5.
    Carry the legacy ANSI escapes verbatim. The exact ungated bytes
    (note the doubled `ESC[36m` seam where `A` joins `F` because the
    skipped comma-prefixed `D` / `M` fragments would otherwise sit
    between them) are:
    `\r\n\x1b[32mMsg. Options: \x1b[33mA\x1b[36m\x1b[36m,\x1b[33mF\x1b[36m,\x1b[33mR\x1b[36m,\x1b[33mL\x1b[36m,\x1b[33mQ\x1b[36m,\x1b[33m?\x1b[36m,\x1b[33m??\x1b[36m,\x1b[32m<\x1b[33mCR\x1b[32m> \x1b[32m(\x1b[0m <range> \x1b[32m )\x1b[0m>: `
  - **Range string** (`amiexpress/express.e:12010`): forward form is
    `{msgNum}+{highest_existing}` — `highest_existing` is
    `mailStat.highMsgNum - 1`, the highest existing message number. So
    reading msg 1 of a 2-message base shows `1+2`; advancing onto msg 2
    shows `2+2`.
  - **`<CR>` / empty input** advances to the next message in the current
    msgbase (re-reads via the same per-message path). **`Q` / `q`**
    returns to the menu prompt. Advancing past `highest_existing` hits
    the legacy out-of-range → `QUIT` clamp (`:12012`) and returns to the
    menu. Any other key in B4 simply re-renders the prompt (real options
    arrive in B5).
  - **Spec**: add the lightweight `messaging.allium` read-sub-prompt
    surface (option set + `D` / `M` / `EH` gates), authored before the
    code.
- **Out of Scope**
  - Options other than `<CR>` / `Q` — added in B5.
  - The no-arg `R` → "first unread" entry — `R` still requires a numeric
    arg here; the no-arg interactive entry is a later refinement.
- **First failing test**: `rust/tests/tierb_read_subprompt_smoke.rs`,
  in-process `TelnetListener` (reuse the `tierb_mail_scan_smoke.rs`
  helpers). Seed a base with two public messages (1, 2); sign in; send
  `R 1`; assert the verbatim ungated skeleton with range `1+2`; send an
  empty line and assert message 2 surfaces with the prompt re-rendered
  at range `2+2`; send `Q` and assert the main conference menu prompt
  returns. Fails today because `R` reads one message and drops straight
  back to the menu.
- **Why split**: ship the legacy primary mail-reading loop with the
  smallest possible surface first; users get the legacy *feel*
  immediately and the remaining options accrete behind it.

## Slice B5 — `R` sub-prompt full options

- **In Scope** — each option dispatches to an **existing** domain rule;
  no new mail rule is written (legacy dispatch at
  `amiexpress/express.e:12092-12224`):
  - `A`gain — re-render the current message (`displayMessage`, `:12102`).
  - `R`eply — `ReplyMail` rule (`:12161`); refined in B6.
  - `F`orward — `ForwardMail` rule (`:12153`); refined in B6.
  - `D`elete — `KillMail` rule (`:12147`), gated `ACS_DELETE_MESSAGE`.
    **This is where the top-level `K` retires** (our `K` = soft-delete =
    legacy `D`elete; legacy `K` is the unrelated "keep and quit", which
    we do not carry).
  - `M`ove — `MoveMail` rule (`:12169`), gated `ACS_SYSOP_READ`.
  - `EH` — `EditHeader` rule (`:12179-12182`), gated `ACS_MESSAGE_EDIT`.
    `EH` is an `E`-family option shown only in the `??` long help
    (`:12051`), not the short skeleton.
  - `L`ist — render B3's scan listing table (`listMSGs`, `:12220`).
  - `?` short help (`:12023-12032`) and `??` long help
    (`:12034-12060`) — both end with the
    `<CR>=Next ( <range> )? ` re-prompt; carry the legacy ANSI verbatim.
  - **Access-gate modelling**: `ACS_SYSOP_READ` → existing `is_sysop()`;
    `ACS_MESSAGE_EDIT` → existing `Right::MessageEdit`; `ACS_DELETE_MESSAGE`
    → reuse whatever gate the current top-level `K` (`KillMail`) already
    checks (confirm during planning — add a `Right` variant only if `K`
    is presently ungated).
  - Existing top-level `RP`, `FW`, `K`, `MV`, `EH` parsers stay live
    through B5 so the B-wire smoke can assert both paths reach the same
    rule; B8 removes them in this same round.
- **Out of Scope** (legacy `readMSG` options NextExpress does not carry
  here, documented so the omission is deliberate):
  - `NS` non-stop mode — Tier A A12.
  - `T` / `TS` / `T!` / `T*` translate (`ACS_TRANSLATION`) — niche;
    no translation subsystem is planned.
  - `U` account-edit-from-mail (`ACS_ACCOUNT_EDITING`) — Tier H sysop
    console territory.
  - `E` / `EM` body editors — deferred with the legacy screen-mode
    editor (see `COMMAND_PARITY.md` row 20); only `EH` (header edit)
    lands now.
  - `K`eep-and-quit (mark-unreceived) — distinct legacy option, not
    carried.

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
    `To` / `Subject` / `Private` prompts, adopting the verbatim "User
    does not exist!!" text, and so on. **Do not** flip the Private
    default: `yesNo(2)` is default-**N** and our prompt already matches
    (the old "swap to Y" note was a misreading; see `COMMAND_PARITY.md`
    row 16).
- **Out of Scope**
  - The legacy screen-mode editor (rows 20 / `edit()`) — keeping the
    NextExpress line-mode editor is a deliberate divergence (see
    `COMMAND_PARITY.md` row 20).

## Slice B-wire — Tier B wire-and-smoke

Runs *after* B8 this round (see "This round" above).

- **In Scope**
  - Smoke test exercises: log in, run `MS`, drop into a message via
    `R 1`, navigate forward with `<CR>`, reply via the sub-prompt,
    return to the menu.
  - Assert the verbatim sub-prompt wire bytes, and that the retired
    top-level shortcuts (`RP` / `FW` / `K` / `MV` / `EH`) now fall
    through to the unknown-command notice.
- **Out of Scope**
  - Re-introducing the top-level shortcuts in any form.

## Slice B8 — Retire top-level `RP` / `FW` / `K` / `MV` / `EH` shortcuts

Pulled forward into this round (see "This round" above): the deprecation
window is collapsed, so B8 lands as soon as B5 has shipped the
equivalent sub-prompt options — no release-cycle wait, no capability gap.

- **In Scope**
  - Drop the `MenuCommand::Reply` / `Forward` / `Kill` / `Move` /
    `EditHeader` variants and their parse arms from `menu_command.rs`
    and their dispatch arms from `menu_flow/mod.rs`. The five commands
    parse to `MenuCommand::Unknown` afterwards (the `N` precedent, B2).
  - **Keep the domain rules** (`ReplyMail`, `ForwardMail`, `KillMail`,
    `MoveMail`, `EditHeader`) — they are now reached only from the
    sub-prompt (B5), not deleted.
  - Existing tests covering the top-level forms move to assert the
    R sub-prompt equivalent. Mind the mapping: top-level `K` retires
    onto the sub-prompt `D`elete; `EH` onto the sub-prompt `EH`.
  - Drop any menu-asset lines (`Conf*/Menu*.txt`) that reference the
    five tokens (likely none — they were never legacy menu commands).
  - Add a `COMMAND_PARITY.md` note (rows 22–26) recording the removal
    so future readers see why typing `RP 1` now falls through to the
    unknown-command notice.
- **Out of Scope**
  - Re-introducing them under a feature flag — by the time this
    slice ships, the sub-prompt is the only legitimate path.

## Slice B9 — Faithful ACS access-flag mapping for the sub-prompt — **Todo**

Reading the original `readMSG` while wiring B5c surfaced a pre-existing
divergence (from Slice 49b, *not* introduced by the sub-prompt round):
our `D` / `M` / `EH` gates do not map to the three distinct legacy
`checkSecurity` flags. The original gates each option on its own flag:

| Option | Legacy flag (`amiexpress/express.e`) | Our gate today |
| --- | --- | --- |
| `D`elete | `ACS_DELETE_MESSAGE` (`:12148`) | `is_sysop \|\| access ≥ 210 \|\| author \|\| addressee` (`delete_mail::can_delete`) |
| `M`ove | `ACS_SYSOP_READ` (`:12170`) | `is_sysop \|\| message_edit` (`move_mail::can_move`) |
| `EH` edit header | `ACS_MESSAGE_EDIT` (`:12179`) | `is_sysop \|\| access ≥ 210` (`edit_mail_header::can_edit_header`) |

So `move` keys off `message_edit` where the legacy uses `SYSOP_READ`,
and `EH` keys off `access ≥ 210` where the legacy uses `MESSAGE_EDIT` —
the two flags are effectively crossed, and `D` is broadened with
ownership disjuncts beyond the legacy's flat flag. (Note: `moveMSG`
itself — `express.e:11827` — performs *no* ownership check, so the
privilege-only shape of `can_move` is faithful; only the *flag* is
wrong.)

- **In Scope**
  - Model `ACS_DELETE_MESSAGE` / `ACS_SYSOP_READ` / `ACS_MESSAGE_EDIT`
    as three distinct rights (or a faithful tier mapping), and re-point
    `can_delete` / `can_move` / `can_edit_header` at them.
  - Decide whether to keep `delete`'s author/addressee ownership
    disjuncts (a NextExpress enrichment) or drop them for legacy parity.
  - Align the `messaging.allium:MailReadPrompt` surface gate names with
    whatever the rules end up checking.
- **Out of Scope**
  - The sub-prompt wiring itself — B5c already gates each option on its
    rule's predicate, so this slice swaps the predicate's *contents*
    without touching the dispatch.
- **Why deferred:** it touches the `Right` enum, the user-tier mapping
  and three domain rules used by the top-level commands too, so it is a
  cross-cutting access-model change rather than sub-prompt work.

## Slice B10 — bare `R` no-arg entry + legacy-exact `readMSG` loop — **Done**

B4 shipped the `R` sub-prompt but deferred the no-arg entry. Landing it
faithfully forced a **legacy-exact rework of the whole `readMSG` loop**
(`amiexpress/express.e:12008-12230`), because bare `R` and `R <num>`
share that loop and the existing B4/B5 shape diverged from legacy in
three ways that only became visible at the no-arg edge. The rework
reshapes the shared loop, so `R <num>` and the `MS` read-it-now flow
changed too. (Original scope was "dispatch wiring + a start clamp"; a
parity-verification workflow against `express.e` and the
`comparison/transcripts/` captures showed the shape itself was wrong,
and the sysop opted for the full rework.)

What shipped (all in `rust/src/app/menu_flow/`):

- **Prompt-first bare `R`.** `MenuCommand::Read(NumberArg::Missing)` no
  longer emits `READ_REQUIRES_NUMBER_LINE`; it opens the sub-prompt
  *before* displaying any message (legacy enters `cont:` directly when
  there are no params, `:11999-12021`). The first `<CR>` then reads the
  resume message. `R <num>` stays read-first (legacy `passItIN`,
  `:12003-12004`). The loop entry is unified:
  `run_read_subprompt(session, next, last_displayed)` where bare `R`
  passes `(start, None)` and `R <num>` / MS pass `(num + 1, Some(num))`.
- **Start = read-pointer + 1, not an unread search.** Legacy
  `msgNum := lastMsgReadConf + 1` (`:11984`, `lastMsgReadConf := cb.confYM`,
  `:4912`) clamped up to the lowest key (`:11985`). Seam:
  `user.read_pointers_for(msgbase).last_read() + 1` clamped up to the
  base's lowest. **Not** `scan_mail::first_unread_number_for` — that
  returns the lowest *unread-addressed-to-me* message, returns `None`
  once mail is read, and would replay message 1 (wrong-seam trap, pinned
  by `bare_r_with_an_exhausted_pointer_returns_to_the_menu_without_replaying`).
- **Next-to-read range numbering.** The range lower bound is the *next*
  message to read, because `readit` increments `msgNum` *after*
  `displayMessage` (`:12372`). So `R 1` on a 2-message base shows
  `( 2+2 )` (not `1+2`), and after reading the last message the range
  collapses to the literal `( QUIT )` (`:12012`). The renderer now takes
  a precomputed `range: &[u8]`; the caller computes the QUIT collapse
  (`next > highest || next < lowest`).
- **`( QUIT )` exhausted prompt.** An out-of-range pointer (exhausted, or
  empty base) renders the prompt with the `QUIT` range rather than
  returning silently. `<CR>` / `Q` there return to the menu silently
  (legacy implicit-advance sets `noDirF = 1`, so `noMorePlus` prints
  nothing, `:12082`/`:12302`). The spurious `Message not found.` that the
  first (message-first) attempt leaked is gone — the loop guards
  `next > highest` before reading.
- **`tempFlag`-inert options.** `A`/`R`/`F`/`D`/`M`/`EH` operate on the
  *loaded* message (`last_displayed`) and are inert until one has been
  read (legacy `IF(tempFlag)`, `:12087`); before the first read only
  `<CR>`/`L`/`Q`/`?`/`??` act.
- **Spec**: the `messaging.allium` `MailReadPrompt` no-arg guidance note
  and `@guarantee NavigationWalksForward` were updated to the prompt-first
  + `QUIT` model.

**Tests** (`rust/tests/tierb_read_subprompt_smoke.rs`): the ~13 existing
B4/B5 smokes were re-pinned to the next-to-read ranges / `QUIT` forms,
and new smokes added — `bare_r_*` (prompt-first, resume, exhausted),
`bare_r_options_are_inert_before_the_first_message_is_read`, and
`help_tail_shows_quit_when_out_of_range`.

- **Out of Scope / deferred** (recorded so the deferral is deliberate):
  - **B9 per-user ACS gating.** Legacy gates the prompt's `D`/`M` on the
    per-user `checkSecurity(ACS_DELETE_MESSAGE)` / `(ACS_SYSOP_READ)`
    flags (`:12017-12018`), so legacy shows them even at the
    `QUIT`-from-start prompt with no current message. NextExpress keeps
    the existing per-message gating and hides `D`/`M` when there is no
    current message. Still tracked as B9.
  - **In-loop digit / `+` / `-` jumps and the `noMorePlus`
    "The last message in this conference is N" text** (`:12238-12304`).
    The current loop has no in-prompt number jump, so the `noDirF = 0`
    text never fires; the `R <num>` out-of-range path still uses the
    existing `Message not found.` divergence (COMMAND_PARITY §1). Deferred.
  - **`S` ("new only") / `NS` (non-stop) tokens** (`:11989`); `NS` is
    Tier A A12.
  - **Lowest-key approximation.** The clamp uses
    `lowest_undeleted_message` (≈ legacy `lowestNotDel`) rather than
    `mailStat.lowestKey` (lowest incl. deleted); they differ only when the
    lowest physical key is a soft-deleted message below the lowest
    undeleted one. Recorded as a narrow documented divergence.
