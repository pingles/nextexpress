# Command Parity — NextExpress vs AmiExpress

Wire-text comparison of every menu command NextExpress currently dispatches
against the equivalent path in the legacy AmiExpress source
(`amiexpress/express.e`). Lives at the repo root so it sits beside
[`SLICES.md`](./SLICES.md) and the design docs — a contributor working a
slice that touches a menu command can use this file to pin the wire
format against the legacy without re-deriving it.

Notation:

- The legacy source uses `\b\n` for `CR LF` (Amiga E convention); on the
  wire that maps directly to `\r\n`. ANSI SGR escapes (`[32m` etc.) are
  written into the same string and emitted verbatim when the user has
  ANSI graphics enabled.
- "—" means nothing is emitted for that path.
- `internalCommandX` references resolve to procedures in
  `amiexpress/express.e`; line numbers are given for every claim.

## 1. Commands NextExpress currently dispatches

| # | Command (form) | AmiExpress experience (source) | Our experience | Verdict / what to change |
|---|---|---|---|---|
| 1 | `G` (logoff) | `internalCommandG` (`express.e:25047`) optionally prompts to confirm flagged-file abandonment; sets `REQ_STATE_LOGOFF`; the listener emits SCREEN_LOGOFF then `Goodbye!\r\n\r\n` (`express.e:17792, 20231`). | SCREEN_LOGOFF (`Screens/LOGOFF.txt`) rendered when present, then `Goodbye!\r\n` (`GOODBYE_LINE`). No flagged-file confirm (no files yet); single trailing CRLF. | **Minor.** Add a second trailing `\r\n` to match. Flagged-file confirm needs Phase 9. SCREEN_LOGOFF wiring ✓ (`ScreenRepository::logoff_screen`, written from the menu-loop Logoff branch before the Goodbye line; absent asset = silent skip, matching the spirit of the legacy `displayScreen` gate). |
| 2 | `J` (no arg) | Displays SCREEN_JOINCONF asset, then prompts `Conference Number (1-N): ` via `lineInput` (`express.e:25143-25151`). Blank input returns to menu silently. | Rejects: `\r\nUsage: J <conference-number>\r\n`. | **Significant gap.** Legacy is *interactive*. New slice: implement the no-arg `J` prompt sub-flow (mirror `lineInput` contract: blank = abort to menu). |
| 3 | `J <invalid token>` | Legacy `Val()` parses non-numeric as `0`, then the prompt re-asks; never surfaces "invalid". | `\r\nInvalid conference number.\r\n`. | Conditioned on (2): silently fall through to the prompt instead of emitting a notice. |
| 4 | `J <num>` no access | `\b\nYou do not have access to the requested conference\b\n\b\n` (`express.e:25157`). | `\r\nYou do not have access to the requested conference\r\n\r\n` (`NO_ACCESS_TO_REQUESTED_CONFERENCE_LINE`). | ✓ Matches verbatim. |
| 5 | `J <num>` join (success) | `[32mJoining Conference[33m:[0m <name>` (`express.e:5083`). | `\r\n[32mJoining Conference[33m:[0m <name>\r\n`. | ✓ Matches. |
| 6 | Auto-rejoin on logon | `\b\nConference <n>: <name> Auto-ReJoined\b\n` (`express.e:5071-5073`). | `\r\nConference <n>: <name> Auto-ReJoined\r\n`. | ✓ Matches. |
| 7 | `R` (no arg) | Enters `readMSG` sub-prompt loop (`express.e:11972`). Shows the ANSI-coloured prompt `Msg. Options: A,F,R,L,Q,?,??,<CR> ( <range> )>: ` assembled at `express.e:12016-12021`, where `<range>` is the runtime message-range string (e.g. `5+10`, built at `:12010`) and the `D` (delete, gated on `ACS_DELETE_MESSAGE` at `:12017`) / `M` (move, gated on `ACS_SYSOP_READ` at `:12018`) options appear after `A` only for callers with those flags. CR advances to next message; `?` shows short help, `??` long. **The R sub-prompt is the legacy's primary mail-reading UI.** | `\r\nUsage: R <message-number>\r\n` — rejects no-arg form. | **MAJOR.** No equivalent interface yet. Either (a) implement the full R sub-prompt with options menu (large; gates K/MV/EH/FW/RP behind R), or (b) keep the simplified surface and document the divergence. Recommended: file as a Phase 9+ slice. |
| 8 | `R <num>` | Reads the message via `displayMessage` then drops into the R sub-prompt. Header block uses ANSI labels `Date / Number / To / Recv'd / From / Status / Subject` (`express.e:8900-8938`). | Reads the message with `render_mail_header` (matching label block) then returns directly to `Command:` prompt. | **Significant.** Header block matches ✓ but the post-read sub-prompt is missing. |
| 9 | `R <num>` not found | Legacy: the read loop iterates past missing message files — no explicit "not found" line. (No `Msg #X not found` string in `express.e`.) | `\r\nMessage not found.\r\n` (`MESSAGE_NOT_FOUND_LINE`). | **Drift / acceptable.** Legacy is silent; we emit a notice. Recommend: keep notice, document divergence. |
| 10 | `R <num>` deleted | `\b\nThat message has been deleted.\b\n\b\n` (`express.e:8890`). | `\r\nThat message has been deleted.\r\n\r\n` (`DELETED_MESSAGE_LINE`). | ✓ Verbatim match (both trailing CRLFs included). |
| 11 | `MS` (scan all) | Bare `M` is the ANSI toggle (`internalCommandM`, `:25239`); scan-all is `MS` (`internalCommandMS`, `:25250`), which prints `\b\nScanning conferences for mail...\b\n\b\n` (`:25258`) then loops `joinConf(..,FORCE_MAILSCAN_ALL)` over every accessible conf/msgbase. Per base, `searchNewMail` (`:11651`) has two **mutually-exclusive** branches: `currentConf=0` (the MS path) prints a column-padded `Type/From/Subject/Msg` header + dashes row + `[0m` (`:11713-11715`) and one row per unread (`:11720`); `currentConf<>0` (single-conf / auto-join) prints **no** table, only `\b\nFound Mail!` (`:11737`). Both end with `\b\nWould you like to read it now ` (`yesNo(1)`, `:11739`). | `MS` (`MenuCommand::ScanAllMail`) walks every accessible conference, printing the `Scanning conferences for mail...` header, a per-conference banner, and the `Type/From/Subject/Msg` listing table per base (or `No mail today!`). | **Resolved (B1 + B3).** Deferred to B4/B5: the `Would you like to read it now ` prompt + drop-into-read (`R` sub-prompt territory). |
| 12 | `N` (scan new) | Legacy `N` is **not** a mail scan: `internalCommandN` (`:25275`) gates on `ACS_FILE_LISTINGS` and tail-calls `myNewFiles` — the new-**files** listing. There is no legacy `N`→mail binding. | `N` is now an **unknown command** — the mail-scan binding (`Scan(ScanArg::New)`) was removed entirely. (`MS = ScanAllMail` — the multi-conference scan; bare `M` = ANSI toggle.) | **Resolved (B2).** The mail-scan drift is gone; the legacy `N` = new-files scan lands in Tier D, which restores `N` and its menu line. |
| 13 | `E` (no arg) | Decoration line `\b\n                       [32m([33m------------------------------[32m)[0m\b\n` then `     [36mTo[33m: [32m([33mEnter[32m)[0m=[32m'[33mALL[32m'[32m?[0m ` (`express.e:9999-10000`). Blank input → addresses ALL. | `\r\nTo: ` — plain, no decoration, no ANSI, no ALL default. | **Significant.** Wire format differs sharply. Update `POST_TO_PROMPT`; implement "blank = ALL" semantics. |
| 14 | `E <to>` | Skips the To prompt, echoes the typed name on its own line via `aePuts(mailHeader.toName); aePuts('\b\n')` (`express.e:10770-10771`). | Skips the To prompt; no echo. | **Minor.** Add the echo so the user sees who their mail will go to before being asked for Subject. |
| 15 | `E` — Subject prompt | `[36mSubject[33m: [32m([33mBlank[32m)[0m=[33mabort[32m?[0m ` (`express.e:10847`). Blank → silent abort. | `Subject: ` then on blank: `\r\nMessage aborted.\r\n`. | **Significant.** Missing ANSI + abort hint; legacy is silent on abort while we emit a notice. Update `POST_SUBJECT_PROMPT`; keep or drop `POST_ABORTED_LINE` as a deliberate choice. |
| 16 | `E` — Private prompt | `         [36mPrivate ` + `yesNo(2)` renders `[32m([33my[32m/[33mN[32m)[32m?[0m ` (i.e. `(y/N)? `) with default **N** — CR maps to "n" (`express.e:10861-10862`; `yesNo` at `:2129`, CR-default at `:2145`). | `Private (y/N)? ` — default **N**. | **Minor (text only).** Default already matches legacy (No / public) — the earlier "swap to Y" note was a misreading of `yesNo(2)`, which is default-**N**, not default-Y. Only gap is the missing ANSI colour. Add ANSI; do **not** change the default. |
| 17 | `E` — unknown user | `\b\nUser does not exist!!\b\n\b\n` (double bang) (`express.e:10814`). | `\r\nUnknown user.\r\n`. | **Drift.** Adopt legacy text verbatim. |
| 18 | `E` — recipient no access | `\b\nUser does not have access to this conference!\b\n\b\n` (`express.e:10838`). | `\r\nUser does not have access to this conference.\r\n`. | **Drift.** Swap `.` for `!` + add trailing blank line. |
| 19 | `E` — addressing not allowed | `\b\nCan't use EALL in external message bases!!\b\n\b\n` (`express.e:10806`). | `\r\nThis message base does not accept that addressee.\r\n`. | **Drift.** Need separate notices per addressing kind (EALL / ALL) matching legacy. |
| 20 | `E` — body editor | Drops into `edit()` line editor (`express.e:10962`). On success prints `Saving...` with no message number. | Custom line-mode editor with `\r\nEnter your message. End with a single '.' on a line by itself; '/A' aborts.\r\n` then `\r\nMessage #N saved.\r\n`. | **Drift / acceptable.** Our editor protocol is *not* legacy parity. The legacy editor is a screen-mode editor with `B>`/`E>`/numeric line commands. Treat ours as a placeholder until Phase 9 lands the legacy editor. Echoing the message number on save is a UX improvement worth keeping. |
| 21 | `C` (comment to sysop) | `commentToSYSOP` prints decoration line + `     [36mTo[33m: [32m([33mEnter[32m)[0m=[32m'[33mALL[32m'[32m?[0m <sysop-name>` (the sysop name appears at the end of the line, pre-filled) then Subject prompt, then routes through `enterMSG` (`express.e:8779-8782`). | Routes directly into post handler with To pre-set; user sees `Subject: ` immediately, never sees the To: line. | **Significant.** Legacy *shows* the sysop name on a printed To: line. Update the flow to print the decoration line + `To: <sysop>` echo before the Subject prompt. |
| 22 | `RP <num>` (reply) | **Not a menu command.** `R>eply` lives inside the R sub-prompt (`express.e:11017, 11039`). | Top-level menu command; drops into a body editor and infers subject `Re: <original>` / addressee from source. | **Architectural divergence.** Acceptable while the R sub-prompt is absent. Document; once R sub-prompt lands, drop the top-level form. |
| 23 | `FW <num>` (forward) | Same as 22 — `F>orward` is an R sub-prompt option. `forwardMSG` (`express.e:9807`) reuses the standard To-header via `msgToHeader()` (the `To: (Enter)='ALL'? ` prompt), then copies the original body after `\b\nSaving...` — there is **no** `Forward to:` string and **no** `edit()` note. | Top-level command; prompts `\r\nForward to: ` then `Optional note. End with a single '.' on a line by itself; blank line skips.\r\n`. | **Architectural divergence + drift.** Our `Forward to:` prompt and note-entry are NextExpress inventions; the legacy reuses its To-header prompt and copies the body with no note. Align in Slice B6. |
| 24 | `K <num>` (kill / delete) | Same as 22 — `D>elete Message` is an R sub-prompt option. Legacy confirms with a `Y/N` prompt. | Top-level; `Delete message (y/N)? ` then on `y`: `\r\nMessage deleted.\r\n`. | **Architectural divergence.** Same disposition. |
| 25 | `MV <num>` (move) | Same as 22 — `M>ove Message` is an R sub-prompt option (sysop only). Legacy prompts for target conf / msgbase. | Top-level; `\r\nTarget conference number: ` then `Target msgbase number: ` then on success `\r\nMessage moved. New number N.\r\n`. | **Architectural divergence.** Same disposition. |
| 26 | `EH <num>` (edit header) | Same as 22 — `EH` is an R sub-prompt option (sysop). | Top-level; `New subject (blank = unchanged): ` then `New To (blank = unchanged): ` then `\r\nHeader updated.\r\n`. | **Architectural divergence.** Same disposition. |
| 27 | Unknown command | The legacy command dispatcher first searches menu tooltype overrides and `BBS:Commands` external commands; if nothing matches, it falls through with no notice and re-prints the menu prompt. There is no `Unknown command.` string in `express.e`. | `Unknown command. Type G to log off.\r\n` (`UNKNOWN_COMMAND_LINE`). | **Drift / acceptable.** Either drop the notice to match legacy or keep it as a usability improvement. Recommend keeping — silently swallowing typos is unfriendly. |

## 2. Wire-format infrastructure differences (cross-cutting)

| Item | AmiExpress | Ours | Verdict |
|---|---|---|---|
| Line terminator | `\b\n` (CR LF) | `\r\n` (CR LF) | ✓ Identical on the wire. |
| ANSI colour escapes | Liberally used in prompts (`[32m` green / `[33m` yellow / `[36m` cyan / `[0m` reset) | Used in header / explicit-join blocks only; missing from most prompts | **Significant.** Add ANSI to To/Subject/Private prompts. |
| Trailing blank lines | Legacy notices often end with `\b\n\b\n` (double CRLF, vertical breathing room) | Mixed — some have it, some don't | **Drift.** Audit and normalise. |
| Yes/No prompt default | `yesNo(1)` → `(Y/n)?` default **Y** (CR→"y", `:2136,2144`); `yesNo(2)` → `(y/N)?` default **N** (CR→"n", `:2134,2145`). `(y/N)?`/default-N is the common case (e.g. Private, `:10862`). | Hard-coded `(y/N)?` everywhere | **Minor.** Default-N already matches the common `yesNo(2)` sites; the gap is ANSI colour and the few `yesNo(1)` (default-Y) sites, e.g. searchNewMail's "read it now" (`:11740`). |
| "Sysop only" denied | (Varies per gate) | `\r\nYou do not have permission to perform that operation.\r\n` | The legacy notice depends on the gate; we use a single string. Acceptable. |
| Source-not-found for K/MV/EH/RP/FW | (n/a — these aren't menu commands in legacy) | `\r\nNo such message in this base.\r\n` | Greenfield text. Keep. |

## 3. Summary

- **Verbatim matches:** auto-rejoin, explicit join, J no-access, deleted-message read, SCREEN_LOGOFF rendering on G.
- **Drift to fix verbatim (text-only, easy):** unknown user (17), recipient no-access (18), Goodbye trailing CRLF (1), Subject prompt's ANSI (15), Private prompt's ANSI (16).
- **Behaviour mismatches (semantic, harder):** Subject blank silent vs notice (15), unknown command silent vs notice (27), `J` / `R` no-arg interactive prompts (2, 7). (The Private default is *not* a mismatch — both sides default No; see row 16.)
- **Major missing surface:** R sub-prompt (7, 8) — the legacy's *primary* mail UI and the natural home for K/MV/EH/FW/RP. **Slice candidate (Tier B, B4–B5).**
- **`MS` multi-conf walk + listing rows (11) — Done (Slices B1 + B3):** `MS` now walks every accessible conference and prints the `Type/From/Subject/Msg` table per base. Deferred to B4/B5: the `Would you like to read it now ` prompt + drop-into-read. `N` rebind off mail (12) — Done (Slice B2): `N` is now an unknown command pending the Tier D new-files scan.
- **Acceptable greenfield divergences:** message-number echo on save (20); source-not-found notices for K/MV/EH (placeholder until the R sub-prompt lands).

## 4. Recommended sequencing

1. **Quick wins** (text-only edits in `wire_text.rs`): items 1, 14, 17, 18, 19 — small diff, brings several notices to verbatim parity.
2. **ANSI gap** (15, 16, plus the cross-cutting ANSI / `yesNo(1)` sites): one slice — add ANSI to the To/Subject/Private prompts. Defaults already match; do not flip them.
3. **R sub-prompt** — substantial; lands the natural home for K/MV/EH/FW/RP. Phase 9 slice or dedicated sub-phase.
4. **Interactive `J` no-arg prompt** (2): one small slice once the JOINCONF asset story is settled.
5. **Slices B1 / B3 — Done:** the `MS` multi-conf walk and its listing table shipped together.

## 5. Methodology

- Source of truth on our side: `rust/src/app/menu_command.rs` (parser),
  `rust/src/app/menu_flow/mod.rs` (dispatch), `rust/src/app/wire_text.rs`
  (byte literals).
- Source of truth on the legacy side: `amiexpress/express.e`,
  procedures starting with `PROC internalCommand…` and the helpers they
  call (`enterMSG`, `readMSG`, `commentToSYSOP`, `searchNewMail`,
  `joinConf`).
- Each row reflects a manual cross-check of the exact byte sequence
  each side emits; behavioural notes (silent vs notice, default Y vs N)
  come from reading the surrounding control flow, not just the prompt
  string.
