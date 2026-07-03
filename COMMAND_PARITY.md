# Command Parity — NextExpress vs AmiExpress

Live-telnet behaviour & wire-format comparison of NextExpress (the Rust port)
against the genuine AmiExpress 5.6.0 binary. This file lives at the repo root so
it sits beside [`SLICES.md`](./SLICES.md) and the design docs — a contributor
working a slice that touches a menu command can use it to pin the wire format
against the legacy. It supersedes the earlier source-derived parity table:
every claim here is checked against **live telnet transcripts of both systems**
(under [`comparison/`](./comparison/)), and the actionable roadmap that table
carried is preserved in [Recommended fixes & sequencing](#recommended-fixes--sequencing)
at the end.

## Introduction

This document compares **NextExpress**, the Rust port of the AmiExpress BBS, against the **legacy AmiExpress 5.6.0** binary it ports. Both systems were exercised **live, over telnet**, and the comparison is grounded in actual on-the-wire transcripts rather than source reading alone:

- **NextExpress** was run from the Rust project on `127.0.0.1:2323`.
- **AmiExpress 5.6.0** was run unmodified inside FS-UAE (Amiga emulation) on `127.0.0.1:6023` (binary identifies itself as `AmiExpress 5.6.0 Copyright (c)2018-2023 Darren Coles`), via the repo's `docker/amiexpress-fsuae` harness configured for **4 telnet nodes** (`NODE_COUNT=4`) so multiple sessions can connect concurrently. The harness's per-IP DoS throttle is disabled for the localhost test (`DOSCHECKTIME=0`); see that directory's entrypoint.

**Method.** A scripted telnet driver drove both systems through the same login and main-menu command sequence, capturing each side's raw wire bytes. NextExpress already ships four seeded messages in conference 1; AmiExpress's bundled conferences were empty, so the driver also **created content live** — it logged in and *posted real messages* through AmiExpress's own line editor (declining the full-screen editor), then read them back — so the comparison shows how the legacy system actually renders a message header, body, message list, and save flow, not just empty-base skeletons. The captures under `comparison/transcripts/` are the primary evidence:

- `rust_sysop.txt` — NextExpress full command battery.
- `amiexpress_sysop.txt` — AmiExpress full command battery.
- `amiexpress_login.txt` — AmiExpress login + full menu screen.
- `amiexpress_post_and_list.txt` — AmiExpress live message *posting* (line editor) + list.
- `amiexpress_read_messages.txt` — AmiExpress live message *read* (header + body).

The `amiexpress/express.e` E source and the Rust `wire_text.rs` / `menu_flow` modules are cited only to explain *why* the wire bytes look the way they do. The driver scripts are under `comparison/harness/`.

**Key caveat — data vs. behaviour.** The two installs carry **different seeded data**: different conference names and numbers (Rust auto-rejoins `Conference 1: Main`, AE `Conference 2: Amiga`), different user records, different clocks, different message bases, and a different configured BBS name (the AE fixture's board name is `"NextExpress Reference"`, which is *not* the software — it is the genuine AmiExpress binary with that board name configured). Consequently this comparison is about **behaviour and wire format**, not data values. Wherever a difference reduces to a seeded name, number, clock, or build string, it is tagged **COSMETIC** and explicitly attributed to seed data.

**Scope of commands.** NextExpress implements a focused subset of the much larger AmiExpress command set: login/menu flow, session info (`VER`, `T`, `S`), session toggles (`Q`, `M`, `X`), help (`?`, `H`, `^`), conference and message-base navigation (`J` with its interactive prompt, `JM`, `<` / `>`, `<<` / `>>` — Tier C), conference scan flags (`CF`), message read (`R` + its read sub-prompt), mail scan (`MS`), mail entry (`E`, `C`), file listings (`F` — the NextScan lister, Tier D), the new-files scan (`N` — the AquaScan date-scan experience, slice D9, byte-pinned to `comparison/transcripts/ae_tierd_newfiles.txt`), zippy search (`Z`), alter-flags (`A` — the `alterFlags` listing + `flagFiles` add/clear prompt loop, slices D6a/D6b, byte-pinned to `comparison/transcripts/ae_tierd_alterflags.txt`), and logoff (`G`). AmiExpress carries many more top-level commands (`ZOOM`, file transfer, door/utility commands, and the full read-sub-prompt verb set) that NextExpress has not yet ported or has deliberately retired.

**Tag legend.** Each finding is tagged **MATCH** (byte-for-byte or behaviourally identical), **COSMETIC** (differs only in wording, spacing, ANSI byte-encoding, or seed data), or **BEHAVIOURAL** (a difference in control flow, an output line present in one and absent in the other, or an interactive step missing on one side). Encoding and interaction divergences are at minimum **BEHAVIOURAL**, never COSMETIC — a byte-encoding difference that survives at the wire level is never a pure presentation choice.

---

## Summary of differences

### Behavioural differences

| Command | NextExpress | AmiExpress |
|---|---|---|
| Login (graphics prompt) | Asks `ANSI Graphics (Y/n)? ` (RIP-less); `n`/`N` selects ASCII and strips ANSI from later screens | Asks `ANSI, RIP or No graphics (A/r/n)? ` at connect — three-way incl. RIP (timed input sets terminal caps) |
| Login (`Authenticated.`) | Emits `Authenticated.\r\n` on correct password | No equivalent; transitions silently into mail scan |
| Login (mail-scan / pagination) | Emits only `No new mail.`; never paginates | `Scanning conferences for mail...` + per-conf stats block + two `(Pause)...Space To Resume` gates |
| Login (user-stats screen) | Renders only the six-row `S` block at login; the fuller Area/Caller/baud/ratio screen is still deferred (slice A11) | Auto-renders full Area/Caller/Security/Uploads/Downloads/Ratio screen |
| `VER` (registration line) | Omits `Registered to ...` entirely (no reg-key concept) | Prints `Registered to NONE.` |
| `S` (lower screen) | Stops after `Msgs Posted` — no baud/CPS/protocol/sysop block, no ratio table | Full screen incl. `Online Baud`...`Sysop Here` + Uploads/Downloads ratio table |
| `S` (lead-in rows) | Always leads with `User Number` | Leads with config-gated `Area Name` + always-present `Caller Num.`; no `User Number` row (USERNUMBER_LOGIN unset) |
| `Q` (security gate) | Toggles unconditionally | Gated behind `checkSecurity(ACS_QUIET_NODE)`; denies for low security |
| `Q` (node broadcast) | Flips in-session flag only | Calls `sendQuietFlag()` to suppress OLM/online messages node-wide |
| `H` (return code) | Returns `Ok(())` on unavailable path | Returns `RESULT_FAILURE` (latent, not wire-visible) |
| `^ <topic>` (pause on hit) | Writes screen bytes, no pause, no trailing newline | `displayFile` + `(Pause)...Space To Resume` + trailing newline |
| `^ <topic>` (sanitisation) | Rejects topics outside `[A-Za-z0-9_-]` (path-traversal guard) | Passes raw param into `help/<param>`, no sanitisation |
| `J` (no/invalid/out-of-range argument) | ~~Rejections + fall-through-to-Main drift~~ **Resolved by Tier C (C2)**: the interactive `Conference Number (1-N): ` single-shot prompt, blank aborts, prompt input clamped, direct args never clamped — byte-matched against the live reference (see [Tier C navigation](#tier-c--conference-and-message-base-navigation)) | Interactive `Conference Number (1-2): ` sub-prompt; blank aborts; prompt input clamped |
| `R <num>` not found | Single `Message not found.` for any bad number | Out-of-range → `The last message in this conference is <high>` (live); mid-base gap → silent |
| `MS` (`Found Mail!`) | Emitted nowhere | Prints `Found Mail!` on single-conf/auto-join path |
| `MS` (pagination) | Builds whole output, flushes once; never pauses | Paginates with `checkForPause()` after each row |
| `E <to>` (inline) | Skips To-echo/box; no visible recipient confirmation | Always echoes `To: ... SYSOP` (visible resolved recipient) |
| `E`/`C` (blank subject) | Writes `Message aborted.` | Aborts silently (bare newline) |
| `C` (recipient display) | Straight to `Subject:`; never shows it is addressed to sysop | Prints decoration box + `To: (Enter)='ALL'? SYSOP` |
| `E` (body editor/save) | Ruler / `Msg. Options: A,C,D,E,L,S,?` editor matches structurally, but skips the `FullScreen Editor (y/N)?` fork and the `F`/`X` file-attach verbs, and prints `Message #N saved.` (not `Saving...Message Number N...done!`); `D`/`E` deferred. Reply/forward keep the `.`/`/A` editor | `FullScreen Editor (y/N)?` fork → ruler/line-number editor + `Msg. Options: A,C,D,E,F,L,S,X,?` save menu; `Saving...Message Number N...done!` |
| `N` | ~~`MenuCommand::Unknown` (new-files binding removed, deferred to Tier D)~~ **Resolved (slice D9):** the AquaScan new-files scan, byte-pinned to the dedicated capture `ae_tierd_newfiles.txt` — see [N — new-files scan](#n--new-files-scan-aquascan-door-slice-d9). Access is ungated, consistent with the ungated `F` (the internal gates `ACS_FILE_LISTINGS`; `ACS_NEW_FILES_SINCE` exists but is unused, `axcommon.e:12`) | Real command — `ACS_FILE_LISTINGS`-gated new-files scan / `AquaScan v1.0` |
| `RP`/`FW`/`K`/`MV`/`EH` (top-level) | Menu **still advertises** all five, but every one is rejected as Unknown (internal inconsistency) | Never top-level; menu and dispatcher agree (they live in the `R` sub-prompt) |
| `G` (plain) | ~~Always logs off immediately~~ **Resolved (slice D5/Ga):** plain `G` runs `checkFlagged()` — with files flagged it prints the live-captured confirm `You have flagged files still not downloaded.` / `Do you leave without them? (y/N)?` (`yesNo(2)`, single-key, default N); `N` returns to the menu, `Y` / `G Y` / an empty flag set log off. Byte-pinned to `comparison/transcripts/ae_tierd_g_confirm.txt`. | Plain `G` runs `checkFlagged()`: with files flagged, confirms (default N → return to menu); with nothing flagged, logs off |
| `G` (side effects) | Emits `saveFlagged()`'s `** AutoSaving File Flags **` banner + `<BEL>` on every `G` logoff, even with nothing flagged (slice D5-banner); persists/clears the per-slot flag set on logoff via the `FlaggedStore` port (slice D5-persist, durable under SQLite); `saveHistory()` + the `dump` partial-downloads file still deferred to the file-transfer slice | Runs `saveFlagged()` (banner + persist) + `saveHistory()` on logoff |
| `VER` / `S` (© encoding) | UTF-8 `\xc2\xa9` (deliberate policy — see AGENTS.md "Wire encoding") | Latin-1 single byte `\xa9` |

### Cosmetic differences

| Command | NextExpress | AmiExpress |
|---|---|---|
| Login (password label) | `PassWord: ` | `Password: ` |
| Login (menu art) | Fixed small ASCII block + figlet "Main Menu"; no "Now attending" trailer | Large per-conference ANSI art ending `Now attending to user: sysop` |
| Login (mins. left) | Prompt shows `0` (seed `time_limit_per_call = 0`) | Prompt shows `599` |
| `VER` (product line) | `NextExpress 0.1.0 (93e5d36) Copyright ©2026 Paul Ingles` | `AmiExpress 5.6.0 (02-Jan-2024) Copyright ©2018-2023 Darren Coles` |
| `VER` (lineage block) | Labelled `Based on Versions:`; folds extra `AmiExpress 5` line | Labelled `Original Version:`; Coles attribution in header |
| `T` (trailing blank) | `\r\nIt is ...\r\n`, menu follows immediately | Trailing blank line after the time (`...\r\n\r\n`) |
| `S` (`Lst Date On`) | `31-May-2026 09:45:19` (no weekday) | `Sun 31-May-2026 10:15:56` (leading weekday) |
| `Q`/`M`/`X` (prompt spacing) | Goes straight into prompt | One extra trailing `\r\n` after every toggle status line |
| `?` (menu content) | Plain `.oO(==[ NextExpress :: MAIN MENU ]==)Oo.` box, uncoloured list | /X ANSI logo + bracketed coloured command grid |
| `H` (unavailable trailer) | Auto-redisplays full menu (expert-off) | Shows only prompt preceded by extra `\r\n` (expert-on) |
| `J <num>` (post-join body) | `No new mail.` | `Total messages` stats block |
| `R <num>` (header extras) | Appends `Conf   : [1] Main` line; RFC3339 dates | No `Conf` line in read view; `formatLongDateTime` long dates |
| `R` `??` (long help) | Omits legacy `NS`/`T`/`K`/`E`/`EM`/`U` entries (intentional subset) | Full verb set |
| `E` (To: prompt) | Bare `\r\nTo: `, no box/ANSI | Boxed ANSI `To` prompt with `(Enter)='ALL'?` hint |
| `E`/`C` (Subject: prompt) | Bare `Subject: ` | ANSI `Subject: (Blank)=abort?` hint |
| `E` (Private prompt) | `Private (y/N)? ` full-line read | ANSI `Private (y/N)?` single-keystroke, echoes `Yes`/`No` |
| `E` (unknown recipient) | `Unknown user.` | `User does not exist!!` |
| Unknown command | `Unknown command. Type G to log off.` + full menu redraw | `No such command!!  Use '?' for command list.` (blank-line framed), prompt only |
| `G` (closing text) | `** AutoSaving File Flags **` + `<BEL>` then `Goodbye!` on every `G` logoff, even with nothing flagged (slice D5-banner) | `** AutoSaving File Flags **` + `<BEL>` + `Click...` |

### Exact matches

- **Login:** name prompt `Enter your Name: `; masked password echo (one `*` per char); auto-rejoin line format `Conference <n>: <name> Auto-ReJoined`; menu-prompt format (identical SGR codes, brackets, `mins. left): ` suffix).
- **`VER`:** the two original-author credit lines (Thomas / Synthetic Technologies, Hodge / LightSpeed) are byte-identical.
- **`T`:** `It is <MM-DD-YY HH:MM:SS>` line — `It is ` prefix and `FORMAT_USA` date/time layout byte-identical.
- **`S`:** the six shared rows (`User Number`, `Lst Date On`, `Security Lv`, `# Times On `, `Times Today`, `Msgs Posted`) — label text, 11-char padding, and per-line `[32m…[33m:[0m ` ANSI all match.
- **`Q` / `M` / `X`:** all status lines byte-identical — `Quiet Mode On`/`Off`, `Ansi Color On`/`Off`, `Expert mode enabled`/`disabled`; expert-mode menu suppression and M's ANSI stripping match in effect.
- **`?`:** expert-mode gating and net "menu is shown" effect identical.
- **`H`:** `\r\n\r\nSorry Help is unavailable at this time.\r\n\r\n` byte-identical.
- **`J <num>`:** success announcement `\r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m <name>` byte-identical (ANSI codes match).
- **`R <num>`:** header block (`Date`/`To`/`Recv'd`/`From`/`Status`/`Subject` labels + ANSI), `Recv'd` `N/A`-vs-timestamp logic, deleted-message notice `That message has been deleted.`, the `Msg. Options:` sub-prompt skeleton (incl. doubled-`[36m` seam and range), `<CR>`/`A`gain/`L`ist/`Q`uit behaviour, and the shared `?`/`??` help entries — all byte-for-byte.
- **`MS`:** scan header, conference banner, message-base sub-line, `No mail today!`, and the full listing table (`Type/From/Subject/Msg` columns + rule + status rows) — all byte-for-byte.
- **`E`/`C`:** default-N private flag, unknown-user rejection, conference-access gating, blank-subject abort *trigger* — same behaviour.
- **`F`:** the NextScan lister matches the captured AquaScan door byte-for-byte across the listing body, pager verbs, prompts, errors and exit tails — modulo the three deliberate branding swaps and the documented COSMETIC items (see [F — File Listings](#f--file-listings-nextscan-vs-the-aquascan-door)).

---

## Login, Post-Login Flow & Menu Prompt

Both systems follow the same skeleton — name prompt -> masked password -> auto-rejoin home conference -> scan for mail -> menu screen -> menu prompt — and the **menu prompt itself is byte-for-byte identical**. The divergences are concentrated in the front-door (graphics question, password-label case) and in how much post-login presentation AmiExpress streams (pagination, a rich stats screen, an elaborate ANSI menu file) versus Rust's terse path.

### Graphics prompt (ANSI/RIP/None)
- **AmiExpress** asks `ANSI, RIP or No graphics (A/r/n)? ` at connect (`express.e:29528`) via a timed `lineInput`; the answer sets `ansiColour` (N=off), `ripMode` (R), `quickFlag` (Q).
- **Rust** asks `ANSI Graphics (Y/n)? ` (`ANSI_PROMPT`, `wire_text.rs`) at the head of `LoginFlow::identify` — after the plain copyright preamble, before the banner/title screen and the name prompt. An `n`/`N` answer turns the terminal's live colour mode off, so the `ColourTerminal` strips ANSI SGR from every subsequent screen (banner included); the default (`Y`/CR) keeps ANSI. RIP is dropped, so the choice collapses to ANSI vs. ASCII (no `ripMode`/`quickFlag`).
- **BEHAVIOURAL (residual).** The interactive step and the colour-capability state are present in both; the residual divergence is the deliberately dropped RIP option and the `(Y/n)` vs. `(A/r/n)` wording (NextExpress is RIP-less by design). The banner-after-graphics ordering rests on the legacy `:29552` source order — the reference install's title screen was empty in the capture, so it is not observable on the live wire.

### Name prompt
- Both: `Enter your Name: ` (AE `namePrompt` default at `express.e:31774` + trailing space; Rust `NAME_PROMPT = b"\r\nEnter your Name: "`).
- **MATCH.**

### Password prompt label
- **AmiExpress:** `Password: ` (`express.e:31778`). (Inconsistently, AE's failure path still says `Invalid PassWord`, `express.e:29209`.)
- **Rust:** `PassWord: ` (`wire_text.rs:19`).
- **COSMETIC.** Capitalisation only; both read one masked password line.

### Password echo
- Both echo exactly one `*` per typed character (AE `serPuts('*')` / `conPuts('*')` at `express.e:1545-1546`; Rust `TerminalEcho::Masked`). A 5-char `sysop` renders `*****\r\n` in both transcripts.
- **MATCH.**

### "Authenticated." line
- **Rust** emits `Authenticated.\r\n` (`wire_text.rs:197`) on the correct-password transition.
- **AmiExpress** has no equivalent (`grep Authenticated express.e` is empty); it transitions silently from password into the mail scan.
- **BEHAVIOURAL.** Extra acknowledgement notice present only in Rust.

### Auto-rejoin
- Both print `Conference <n>: <name> Auto-ReJoined\r\n` (AE `express.e:5073`; Rust `format_auto_rejoin_line`). Transcripts show `Conference 2: Amiga Auto-ReJoined` (AE) vs `Conference 1: Main Auto-ReJoined` (Rust) — differing values are **seed data**, not an implementation difference.
- **MATCH** (format).

### Mail-scan-on-login & pagination
- **AmiExpress** prints `Scanning conferences for mail...` (`express.e:25258`), then paginates with `\x1b[32m(\x1b[33mPause\x1b[32m)\x1b[34m...\x1b[32mSpace To Resume\x1b[33m: \x1b[0m` (`express.e:5144`) — **two** such pauses in the transcript — and renders a per-conference stats block (`Total messages` / `Last message auto scanned` / `Last message read`).
- **Rust** emits only `No new mail.\r\n` (`render_scan_summary`, `wire_text.rs:585`) and **never paginates**; the whole login streams to the menu prompt.
- **BEHAVIOURAL.** Different scan output (header + counters vs one line) and AE's `(Pause)` gates are interactive steps Rust lacks.

### Rich user-stats / ratio screen
- **AmiExpress** auto-renders a full Area Name / Caller Num / Security Lv / #Times On / Online Baud / Protocol / Sysop Here block plus an Uploads/Downloads/Bytes-Avail/Ratio table during login.
- **Rust** renders only the six-row `S` block (`render_stats_screen`) at login — after the auto-rejoin mail scan, before the menu (`session_driver.rs::run`). The fuller Area Name/Caller Num lead-in, the Online Baud/CPS/Protocol/Sysop block, and the Uploads/Downloads ratio table are still deferred (slice A11).
- **BEHAVIOURAL.** Rust renders only the shared six-row subset at login, not AE's fuller screen.

### Menu screen (art)
- Both gate the menu art on expert mode (AE `express.e:28583`; Rust `menu_flow/mod.rs:80`).
- **AmiExpress** loads a large configurable per-conference ANSI art file via `displayScreen(SCREEN_MENU)` -> `displayFile` (`express.e:6560,28586`), ending with `Now attending to user: sysop` between `- -- --- ---` rules.
- **Rust** embeds a fixed small ASCII block (`.oO(===[ NextExpress :: MAIN MENU ]===)Oo.` + figlet "Main Menu" + a plain-text command list) and has **no** "Now attending" trailer.
- **COSMETIC.** Both are "the menu screen" shown to non-expert users; content/styling differs and Rust omits the "Now attending" line.

### Menu prompt (mins. left)
- Both render `\x1b[0m\x1b[35m<bbsName> \x1b[0m[\x1b[36m<n>\x1b[34m:\x1b[36m<name>\x1b[0m] Menu (\x1b[33m<mins>\x1b[0m mins. left): ` (AE `displayMenuPrompt` `express.e:28417-28419`; Rust `render_menu_prompt` `wire_text.rs:920`). Same `ESC[35m/36m/34m/33m` codes, same brackets, same `mins. left): ` suffix.
- The minutes value differs (AE `599`, Rust `0`). Both compute `(timeTotal - timeUsed)/60`; the Rust default sysop seed leaves `time_limit_per_call = Duration::ZERO` (`usage_accounting.rs:40`, `seed.rs`), so the prompt shows `0`. The rendering and arithmetic are identical.
- **MATCH** (format); the differing number is **COSMETIC** (seed data), not behavioural.

---

## VER (version)

**Behaviour.** Both BBSes respond to `VER` by echoing the command, emitting a leading blank line, printing a multi-line version/lineage banner, then returning directly to the menu prompt (no pause or "continue?" gate). The banners are plain text — neither wraps any line in ANSI colour. The two original-author credit lines are byte-identical between implementations.

**Wire-format comparison.**

AmiExpress (`amiexpress_sysop.txt:116-124`, source `express.e:25688-25698`):
```
VER\r\n
\r\n
AmiExpress 5.6.0 (02-Jan-2024) Copyright \xa92018-2023 Darren Coles\r\n
\r\n
Original Version:\r\n
  (C)1989-91 Mike Thomas, Synthetic Technologies\r\n
  (C)1992-95 Joe Hodge, LightSpeed Technologies Inc.\r\n
\r\n
Registered to NONE.\r\n
\r\n
```

NextExpress (`rust_sysop.txt:121-127`, source `wire_text.rs:102-116`):
```
VER\r\n
\r\n
NextExpress 0.1.0 (93e5d36) Copyright \xc2\xa92026 Paul Ingles\r\n
\r\n
Based on Versions:\r\n
  AmiExpress 5 Copyright \xc2\xa92018-2023 Darren Coles\r\n
  (C)1989-91 Mike Thomas, Synthetic Technologies\r\n
  (C)1992-95 Joe Hodge, LightSpeed Technologies Inc.\r\n
\r\n
```

**Differences.**

- **COSMETIC — lead product line.** AE: `AmiExpress 5.6.0 (02-Jan-2024) Copyright ©2018-2023 Darren Coles`; Rust: `NextExpress 0.1.0 (93e5d36) Copyright ©2026 Paul Ingles`. Same `Product Version (Build) Copyright ©Years Author` shape; the values differ only because these are different products with different seed/build data. Not an implementation difference.

- **COSMETIC — lineage label & structure.** AE labels the block `Original Version:` and keeps the Darren Coles / AmiExpress attribution in the header line. Rust labels it `Based on Versions:` and folds an extra `  AmiExpress 5 Copyright ©2018-2023 Darren Coles` line in as the first lineage entry. The credited parties (Coles, Thomas, Hodge) and years are the same; only the grouping and label wording change, so the behaviour (display lineage) is equivalent.

- **BEHAVIOURAL — © encoding (resolved by wire-encoding policy).** AE emits a bare `\xa9` (Amiga/Latin-1 single byte; RENDER shows `⟨a9⟩`). Rust emits `\xc2\xa9` (UTF-8 encoding of U+00A9; RENDER shows `⟨c2⟩⟨a9⟩`). The glyph © is identical; the on-wire byte encoding is not — this is a BEHAVIOURAL difference for any client decoding at the byte level. The departure is deliberate and policy-mandated: see AGENTS.md "Wire encoding". The summary table above is tagged accordingly.

- **BEHAVIOURAL — registration status line.** AE prints `Registered to NONE.` from `internalCommandVER` (`express.e:25696-25697`, `StringF(...,'Registered to \s.\b\n',regKey)`). Rust **omits this line entirely** — `VERSION_BANNER` has no registration field and the banner ends after the author lines plus a trailing blank line. NextExpress has no registration-key concept, so this is a feature present in AE and absent in Rust. The `wire_text.rs` doc comment states the elision is deliberate (per `slices/cmds-quickwins.md` A2 "Out of Scope"). Classified BEHAVIOURAL because a line of output present in one system is absent in the other, not merely reworded.

**Verdict: mixed** — the banner framing is cosmetic; the © encoding is a deliberate behavioural departure resolved by the wire-encoding policy (AGENTS.md); the missing `Registered to` line is a deliberate behavioural omission.

---

## T (time) and S (user stats)

Legacy procedures: `internalCommandT()` (`amiexpress/express.e:25622-25644`) and `internalCommandS()` (`amiexpress/express.e:25540-25606`). Rust: `render_time_line` / `render_stats_screen` in `rust/src/app/wire_text.rs:802` and `:851`.

> Note on the transcripts: the files labelled `ae_*.txt` are the genuine AmiExpress 5.6.0 binary; their banner reads "NextExpress Reference" only because that is the BBS name configured in the legacy install, not because they are the Rust port. The Rust port's transcript is `rust_sysop.txt` (distinct ASCII-art Main Menu).

### T — current time

**Behaviour:** both echo the command, emit a `It is <date> <time>` line, then redisplay the prompt. **Wire format matches.** Both render the legacy `FORMAT_USA` two-digit-year layout `MM-DD-YY HH:MM:SS`:

- Rust: `b"T\r\n\r\nIt is 05-31-26 10:02:42\r\n…"`
- AmiExpress: `b'T\r\n\r\nIt is 05-31-26 10:19:37\r\n\r\n…'`

The `It is ` prefix and date/time literal are byte-identical (differing values are just different clocks). Rust's `TIME_FORMAT` `[month]-[day]-[year repr:last_two] [hour]:[minute]:[second]` reproduces the legacy `DateToStr`/`FORMAT_USA` output. The legacy `aePuts('\b\nIt is …\b\n')` backspace+newline pair collapses to a single CRLF on the wire on both sides.

- **COSMETIC:** AmiExpress emits a trailing blank line after the time (`…10:19:37\r\n\r\n` before the prompt, from the closing `\b\n` at `express.e:25640`); Rust's `render_time_line` returns `\r\nIt is …\r\n` with no trailing blank, so the menu follows immediately. Vertical-spacing only.

### S — user stats

**Behaviour:** Rust renders a compact six-row block; AmiExpress renders a full stats screen plus an Uploads/Downloads ratio table. **The shared rows match exactly; AE has many rows Rust lacks.**

Shared rows (byte-identical label text, 11-char column padding, and ANSI):

```
\x1b[32mUser Number\x1b[33m:\x1b[0m …   (Rust always; AE only when USERNUMBER_LOGIN tooltype set — absent in these AE captures)
\x1b[32mLst Date On\x1b[33m:\x1b[0m …
\x1b[32mSecurity Lv\x1b[33m:\x1b[0m …
\x1b[32m# Times On \x1b[33m:\x1b[0m …
\x1b[32mTimes Today\x1b[33m:\x1b[0m …
\x1b[32mMsgs Posted\x1b[33m:\x1b[0m …
```

Every shared label's wording, the trailing-space pad on `# Times On `, the leading/trailing CRLF framing, and the per-line `[32m…[33m:[0m ` ANSI wrapper match the legacy `StringF` templates (`express.e:25551-25569`). The `Lst Date On` value uses the same `DD-Mon-YYYY HH:MM:SS` core on both.

Differences:

- **COSMETIC:** `Lst Date On` — AE prepends the abbreviated weekday (`Sun 31-May-2026 10:15:56`); Rust omits it (`31-May-2026 09:45:19`). Same date/time core, leading `Sun ` only.
- **BEHAVIOURAL:** Rust omits the entire lower half of the legacy screen — `Online Baud`, `Rate CPS UP`, `Rate CPS DN`, `Screen  Clr`, `Protocol`, `Sysop  Here` (`express.e:25571-25597`), the conditional `Credit Account` / `Sysop Pages Remaining` rows, and the full `fileStatus(1)` Uploads/Downloads ratio table (`Conf / Files / Bytes` up & down / `Bytes Avail` / `Ratio`, with `Infinite`/`DSBLD` markers). Rust's doc-comment defers these to "slice A11". Evidence — AE: `…Sysop  Here\x1b[33m:\x1b[0m NO\r\n\r\n\x1b[32m              Uploads                 Downloads\r\n…\x1b[33m       2\x1b[0m> …\x1b[31mDSBLD`; Rust stops after `Msgs Posted` and goes straight to the menu.
- **BEHAVIOURAL:** leading-row divergence. Rust always leads with `User Number`; AE (with `USERNUMBER_LOGIN` unset, as captured) has no `User Number` row and instead leads with the config-gated `Area Name  : <conf>` and always-present `Caller Num.: <callerNum>` rows (`express.e:25550-25559`), which Rust does not emit at all.
- **BEHAVIOURAL (flow context):** in the legacy login/auto-rejoin path the S screen is long enough to feed the `(Pause)...Space To Resume` pager; Rust's six-row S never paginates.

**Verdict:** T = match (one cosmetic spacing nit). S = behavioural — the shared rows are an exact wire match, but Rust implements only a subset of the legacy screen (missing Area Name/Caller Num. lead-in, the baud/CPS/protocol/sysop block, and the ratio table).

---

## Session Toggles: Q (quiet), M (ANSI), X (expert)

All three toggles emit byte-identical status lines in both systems. Rust's `wire_text.rs` constants are direct ports of the `express.e` `aePuts` literals, with the Amiga `\b\n` backspace-newline mapped to the telnet `\r\n`. The observable behavioural effects (expert-mode menu suppression, M's ANSI stripping) also match. Two latent behavioural gaps exist in Rust's `Q` but are invisible in the sysop transcripts because the sysop passes all gates.

### Q — Toggle quiet mode

**Wire format (matches exactly):**
- On — Rust `b'Q\r\n\r\nQuiet Mode On\r\n...'`; AE `b'Q\r\n\r\nQuiet Mode On\r\n\r\n...'`
- Off — Rust `b'Q\r\n\r\nQuiet Mode Off\r\n...'`; AE `b'Q\r\n\r\nQuiet Mode Off\r\n\r\n...'`

The status strings `Quiet Mode On` / `Quiet Mode Off` are identical. Source: legacy `express.e:25509/25511`; Rust `wire_text.rs` `QUIET_MODE_ON_LINE`/`QUIET_MODE_OFF_LINE`.

- **BEHAVIOURAL:** Legacy `internalCommandQ` (`express.e:25505`) gates the toggle behind `checkSecurity(ACS_QUIET_NODE)` and returns `RESULT_NOT_ALLOWED` when denied. Rust (`menu_flow/mod.rs:167`) toggles unconditionally — no security gate. Sysop (level 255) passes either way, so the wire output is identical here; divergence would only show for a lower-security user.
- **BEHAVIOURAL:** Legacy also calls `sendQuietFlag(quietFlag)` (`express.e:25507`) to broadcast the flag to the node (suppressing OLM/online messages). Rust flips only the in-session flag; the broadcast effect is explicitly deferred (comment at `mod.rs:171`). Not observable in a single-session capture.

### M — Toggle ANSI colour

**Wire format (matches exactly):**
- On — AE `b'M\r\n\r\nAnsi Color On\r\n\r\n...'`; Rust `ANSI_COLOR_ON_LINE = b"\r\nAnsi Color On\r\n"`
- Off — AE `b'M\r\n\r\nAnsi Color Off\r\n\r\n...'`; Rust `ANSI_COLOR_OFF_LINE = b"\r\nAnsi Color Off\r\n"`

The status strings match (note the American spelling "Color" preserved from the legacy). The Rust command battery (`rust_sysop.txt`) does not exercise `M`, but the source constants are confirmed.

**EFFECT matches:** After M turns colour off, the immediately following prompt is rendered with *no* SGR escapes — AE REPR line 203 shows the plain `NextExpress Reference [2:Amiga] Menu (599 mins. left): `. Rust's `ColourTerminal` decorator (`colour_terminal.rs:32/79`, `strip_ansi_sgr`) strips SGR from every subsequent write while colour is off, reproducing the same behaviour. Source: `express.e:25241-25247`.

### X — Toggle expert mode

**Wire format (matches exactly):**
- Enabled — Rust `b'X\r\n\r\nExpert mode enabled\r\n...'`; AE `b'X\r\n\r\nExpert mode enabled\r\n\r\n...'`
- Disabled — Rust `b'X\r\n\r\nExpert mode disabled\r\n...'`; AE `express.e:26115`

The status strings `Expert mode enabled` / `Expert mode disabled` match. Source: legacy `express.e:26113`; Rust `wire_text.rs` `EXPERT_MODE_ENABLED_LINE`/`EXPERT_MODE_DISABLED_LINE`.

**EFFECT matches:** In expert mode the menu loop stops auto-printing the full menu before each prompt and `?` is what redisplays it. Rust gates this at `menu_flow/mod.rs:80` (auto-display) and `mod.rs:213` (`?` only redraws in expert mode); the legacy gate is `displayMenuPrompt` at `express.e:28583`. Both transcripts confirm: after X enabled, subsequent AE commands return straight to the prompt with no menu (AE lines 113-214), and Rust's X-on capture (line 356) shows the bare prompt while `?` after X-on (line 398) redraws the whole menu.

### Cross-cutting cosmetic note

- **COSMETIC:** AE emits one extra trailing `\r\n` (blank line) after every toggle status line before re-issuing the prompt, whereas Rust goes straight into the prompt. This is AE's global prompt-spacing convention, not specific to Q/M/X. Compare Rust `...enabled\r\n\x1b[0m...` vs AE `...enabled\r\n\r\n\x1b[0m...`.
- **COSMETIC:** The prompt's BBS-name label text differs by seeded config only ("NextExpress Reference" in the AE Docker fixture vs "NextExpress" in the Rust fixture); both wrap the name in identical `\x1b[35m...\x1b[0m` framing and use the identical `[<n>:<conf>] Menu (<mins> mins. left): ` structure with the same SGR colours.

---

## `?`, `H`, `^` — Menu Redisplay & Help

All three are real, dispatched commands in **both** systems. Note up front: the AE menu art screen advertises neither `H` nor `^`, but both are still handled by AE's command dispatcher (`express.e:28346` for `H`, `:28394` for `^`). The framing that "AE has no such caret command" and that `H` may not be an AE command is contradicted by the source — both exist in AE.

### `?` — Re-display menu (MATCH, behaviour)

Both systems gate `?` on expert mode identically:

- **AE** `internalCommandQuestionMark` (`express.e:24594-24599`): `IF loggedOnUser.expert="X"` then `displayScreen(SCREEN_MENU)`. Outside expert mode it is a no-op — because in non-expert mode the menu is already redrawn before every prompt.
- **Rust** `MenuCommand::ShowMenu` (`menu_flow/mod.rs:208-217`): `if session.user().expert_mode()` then `render_menu_screen`. The menu loop auto-displays the menu before the prompt when `!expert_mode()` (`mod.rs:80-82`).

Net user-visible effect is the same in both modes: `?` results in the menu being shown. Live evidence — Rust shows the full menu after `?` in expert-off (`rust_sysop.txt:77-112`) and expert-on (`:361-396`); AE shows the full menu after `?` with expert on (`amiexpress_sysop.txt:219-242`).

**COSMETIC:** the *content* of the redisplayed menu differs completely — AE is the /X ANSI logo + bracketed command grid (`\x1b[31m[\x1b[33mR\x1b[31m]\x1b[34m.......\x1b[32mrEAD MESSAGES`, `amiexpress_sysop.txt:221-240`); Rust is a plain `.oO(==[ NextExpress :: MAIN MENU ]==)Oo.` box with a categorised, uncoloured list (`rust_sysop.txt:78-111`). This is a screen-asset difference, not control flow.

### `H` — Help (MATCH, wire-level)

Both try an on-disk help asset first (AE `BBSHelp` via `findSecurityScreen`, `express.e:25079-25081`; Rust `bbs_help_screen`), and on a miss emit the **byte-identical** line:

```
\r\n\r\nSorry Help is unavailable at this time.\r\n\r\n
```

(AE `amiexpress_sysop.txt:257`; Rust `rust_sysop.txt:488`; Rust's `HELP_UNAVAILABLE_LINE` at `wire_text.rs:122-123` is a verbatim port of `express.e:25083`.)

**COSMETIC:** What follows the line differs — Rust (tested expert-OFF) auto-redisplays the full menu; AE (tested expert-ON) shows only the prompt preceded by an extra `\r\n`. This is the same expert-gating both share, not an `H`-specific difference.

**BEHAVIOURAL (latent, not wire-visible):** AE returns `RESULT_FAILURE` on the unavailable path (`express.e:25084`); Rust returns `Ok(())` (`mod.rs:285-286`). No observable transcript difference.

### `^ <topic>` — Topic help (MIXED)

Exists in both (AE `internalCommandUpHat`, `express.e:25089-25111`; Rust `MenuCommand::TopicHelp`, `mod.rs:218-227`). Both do the same lookup: try `help/<topic>`, and on a miss truncate the topic one character at a time, retrying, until a screen matches; a total miss is a **silent no-op**. Live Rust confirms the silent miss for both `^ test` (`rust_sysop.txt:493`) and `^ zzznope` (`:535`) — no topic body, just the echoed input and the auto-redisplayed menu (no `help/` screens are seeded). AE's transcript has no `^` rows, so the AE side is source-only here.

**BEHAVIOURAL:**

1. *No pause on a hit.* AE does `displayFile(screen); doPause(); aePuts('\b\n')` after a successful match (`express.e:25097-25101`) — i.e. a `(Pause)...Space To Resume` prompt and a trailing newline. Rust just writes the screen bytes (`mod.rs:224-226`), no pause, no trailing newline; the user drops straight back to the menu. Not exercised on a real hit in either live transcript.
2. *Topic sanitisation.* Rust rejects any topic outside `[A-Za-z0-9_-]` as a silent no-op (`is_safe_topic_help_name`, `file_screen_repository.rs:287-313`) — a path-traversal guard. AE passes the raw param straight into the `help/<param>` path (`express.e:25094`) with no sanitisation. Divergent only for hostile/punctuated topics; benign for normal alphanumeric ones.

---

## J — Join Conference

The `J` command switches the caller's active conference. The successful-join wire format matches exactly between the two systems, and **as of Tier C (slice C2) the error/edge paths match too**: the one-line rejections and the fall-through-to-Main drift documented below were replaced by the legacy interactive prompt — see [Tier C navigation](#tier-c--conference-and-message-base-navigation) for the live-vs-live comparison. Legacy reference: `express.e` `internalCommandJ` (~25113) and `joinConf` (~4975, announcement at 5083). Rust: `app/menu_flow/mod.rs` (Join arm), `app/menu_flow/join.rs`.

### `J <num>` (successful join) — MATCH

Both emit an identical announcement. The legacy `StringF` at `express.e:5083` is `'[32mJoining Conference[33m:[0m \s'`; the Rust constant in `wire_text.rs:972` is `b"\r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m "`. The ANSI codes match byte-for-byte: green `ESC[32m` label, yellow `ESC[33m` colon, reset `ESC[0m` before the name.

- Rust (transcript 662): `J 2\r\n\r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m Programming\r\n`
- AE (transcript 289): `J 2\r\n\r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m Amiga\r\n`

The differing conference names (Programming vs Amiga) and the post-join body (Rust `No new mail.` vs AE's `Total messages` stats block) come from the different seeded data and the mail-scan-on-join path, not from a format difference. **COSMETIC** (data/line-ending only).

### `J` with no / invalid / out-of-range argument — MATCH (Tier C, slice C2)

The pre-Tier-C drift (a `Usage: J <conference-number>` rejection for bare `J`, a NextExpress-only `Invalid conference number.` for `J abc`, and a no-access notice followed by a **silent fall-through join of Main** for `J 99`) is gone. Both systems now run the same flow (`express.e:25142-25158`): any argument whose `Val` is missing or outside `1..numConf` opens the `SCREEN_JOINCONF` asset (nothing, when not installed) and the single-shot `Conference Number (1-N): ` prompt; blank input aborts silently with one CRLF; prompt input is `Val`ed and **clamped** into range (`:25153-25154`); an in-range conference failing the access check prints `You do not have access to the requested conference` and stays put — there is no fall-through join and no logoff. Direct arguments are **never** clamped — they prompt instead. The prompt/clamp/abort paths are verified live on both systems (`ae_tierc{,2,3,4}.txt`, `rust_tierc.txt`); the denied-stays-put arm is not observable live (the reference sysop holds access to every conference) and is pinned from source (`express.e:25156-25158`) plus unit tests and the smoke's revoked-conference fixture. Details and byte quotes in [Tier C navigation](#tier-c--conference-and-message-base-navigation).

---

## Tier C — conference and message-base navigation

Live-vs-live comparison of the Tier C navigation surface: AmiExpress captures `comparison/transcripts/ae_tierc.txt`, `ae_tierc2.txt`, `ae_tierc3.txt`, `ae_tierc4.txt`; NextExpress capture `comparison/transcripts/rust_tierc.txt` (the compiled binary on the repo's seed config, two single-base conferences — directly comparable to the reference's two single-base conferences). Multi-base behaviour is not observable on the reference install (its conferences carry no `NMSGBASES` tooltype), so those flows are pinned from the legacy source and exercised end-to-end by `rust/tests/confnav_smoke.rs`.

### The `Conference Number (1-N): ` prompt — MATCH

Trigger set, observed identically on both systems: `J`, `J 99`, `J 0`, `J abc`, `J -1`, `J +2` all yield exactly `<echo>\r\nConference Number (1-N): ` (no blank line, nothing before the prompt when no `JoinConf` screen is installed). At the prompt: blank → `\r\n\r\n` + menu prompt, conference unchanged; `99` → clamps to the top conference; `0` / `abc` → clamp to conference 1; `2abc` → conference 2 (`Val` digit-prefix); a leading `+` is not a `Val` sign (AE live: `J +2` prompts, `ae_tierc4.txt`). Rust reproduces each captured case byte-for-byte (`rust_tierc.txt`; the `2abc`-typed-at-the-prompt variant was captured on AE only — the Rust side shares the same `val_prefix` path as the direct form, pinned by unit tests).

### `<` / `>` (prev / next conference) — MATCH

In-range hop: join output byte-identical to a direct `J <n>` (AE: `>` from conf 1 joined conf 2, `<` from conf 2 joined conf 1 — `ae_tierc.txt` / `ae_tierc3.txt`; Rust identical). At either edge both systems fall into the `Conference Number (1-N): ` prompt — no wraparound (`express.e:24540/:24559`); blank abort stays put. The walk skips inaccessible conferences (source `:24536-24538`; pinned in the Rust smoke with a revoked middle conference). The legacy `ACS_JOIN_CONFERENCE` gate is not ported (the port has no join right yet; `J` does not gate either) — **BEHAVIOURAL (latent)**, invisible for the all-access sysop.

### `<<` / `>>` / `JM` on single-base conferences — MATCH

Every non-dotted form (`<<`, `>>`, `JM`, `JM 1`, `JM 9`, `JM abc`) prints exactly `\r\nThis conference does not contain multiple message bases\r\n\r\n` and neither joins nor prompts (AE `ae_tierc.txt`; legacy probe `express.e:25211-25215`; Rust byte-identical, `rust_tierc.txt`). NextExpress equates the legacy "`NMSGBASES` tooltype absent" with a single-base conference; the legacy nuance of an explicitly-set `NMSGBASES=1` (which prompts `(1-1)` instead) is deliberately not modelled — **COSMETIC** (configuration nuance with no counterpart in file-based config; recorded in `slices/cmds-conf-nav.md`).

### Dotted / two-token arguments — MATCH

`J 2.1` joins conference 2; `JM 1.1` delegates to `J` and joins conference 1 (both live on both systems). `J 1 2` (base 2 of single-base conference 1) opens `Message Base Number (1-1): ` — observed byte-identically on both (`ae_tierc2.txt` EXTRA, `rust_tierc.txt`). The answer to *that* prompt goes to the join unclamped, where an out-of-range base resets to the primary (`express.e:25179` + `:4995`) — `JM`'s own prompt clamps into `[1,N]` instead (`:25233-25234`). The asymmetry is source-pinned and covered by unit tests plus the smoke; the reference cannot demonstrate it live (no multi-base conference).

### Residual cosmetic deltas

The post-join body still differs (`No new mail.` vs AE's `Total messages` stats block — the known Tier-A-era divergence, deferred with the mail-stats slice), and the menu-screen redraw depends on the seeded expert flag. AE's menu loop also writes one `\b\n` before *every* menu prompt (`express.e:28589`), so e.g. the single-base notice reads `…\r\n\r\n\r\n` + prompt on AE vs `…\r\n\r\n` + prompt on the port — the same global prompt-spacing convention already tagged COSMETIC for the Tier A toggles. All of these pre-date Tier C and apply to every command equally.

---

## R (Read Message) and the readMSG Sub-Prompt

The `R <num>` read path plus its post-read sub-prompt is one of the most faithful ports in the system: the message-header block, the `Msg. Options:` skeleton (now prompt-first for bare `R`, with the next-to-read range and `( QUIT )` collapse — slice B10), the `?`/`??` help lists and the `L`ist view all reproduce the legacy `express.e` strings byte-for-byte (modulo data values). One real behavioural gap remains at the edges — the `R <num>` not-found notice (bare-`R` entry was closed in slice B10).

> Note on the test setup: the legacy transcript is genuine **AmiExpress 5.6.0** ("Running AmiExpress 5.6.0 Copyright (c)2018-2023 Darren Coles", `amiexpress_login.txt:12`); "NextExpress Reference" there is merely the *board name* configured on that AmiExpress instance, not the software. AmiExpress's bundled conferences started empty, so the driver **posted a real message live** through AmiExpress's line editor and read it back: `amiexpress_read_messages.txt` carries the live header + body + sub-prompt for message 1, and `amiexpress_post_and_list.txt` carries the live editor / save / list flow. The read view below is therefore a **live-vs-live** comparison.

### R `<num>` (read an existing message)

**Behaviour:** both systems run `displayMessage` (the header + body) and then drop into the readMSG sub-prompt. Matches.

**Header block — labels + ANSI MATCH; layout differs.** Both label the header `Date / Number / To / Recv'd / From / Status / Subject` with the same `[32m`label`[33m:`colon`[0m`value colouring. Live evidence (both reading a *private* message addressed to the sysop):

AmiExpress (`amiexpress_read_messages.txt`, live read of the message the driver posted):
```
Date   : Sun 31-May-2026 10:47:54         Number: 1
To     : SYSOP                            Recv'd: Sun 31-May-2026 10:50:32
From   : SYSOP                            Status: Private Message
Subject: Comparison harness: private no

This is a PRIVATE message addressed to sysop.
Posted live via the comparison harness to show
how AmiExpress renders message bodies.
```
NextExpress (`rust_sysop.txt`, reading seeded message 1):
```
Date   : 2026-05-12T21:48:50.679071Z  Number: 1
To     : sysop  Recv'd: 2026-05-12T21:48:56.974019Z
From   : sysop  Status: Private Message
Subject: hello
Conf   : [1] Main

Hello
This is a test
```

The label set, the `Recv'd` `N/A`-vs-timestamp logic (Rust live msg 2 EALL → `N/A`, msg 1 received → timestamp; legacy `:8920-8926`), the `Status` wording (`Private Message`), and the body-then-sub-prompt flow all match. The divergences:

- **COSMETIC (layout):** AmiExpress **column-aligns** the right-hand fields — `Number` / `Recv'd` / `Status` start at a fixed offset (~col 42), the left value space-padded to fill. NextExpress packs them with a fixed two-space gap (`<value>  Number:`), so the second column floats. Same labels, different alignment.
- **COSMETIC (truncation):** AmiExpress truncates header values to fixed widths — the live `Subject` shows `Comparison harness: private no` (clipped at 30 chars from `…private note`); the `To`/`From` names are padded to a 30-char column. NextExpress does not truncate (`Subject: hello`).
- **COSMETIC (dates):** Rust dates are RFC3339 with microseconds (`2026-05-12T21:48:50.679071Z`); AmiExpress uses `formatLongDateTime` weekday long dates (`Sun 31-May-2026 10:47:54`). Same labelled/coloured field, different value format.
- **COSMETIC (extra line):** Rust appends a `Conf   : [1] Main` line that the legacy read view omits (legacy only prints `Conf` in the QWK export path, `:26470`).

### R (no argument) — MATCH (slice B10)

- Rust: bare `R` opens the readMSG loop **prompt-first** at the caller's resume point and shows the live sub-prompt — no message is displayed before the prompt; the first `<CR>` reads the resume message (`handle_read_mail_at_pointer`, `read_mail.rs`). The resume start is `read_pointers_for(...).last_read + 1` clamped up to the base's lowest key (legacy `msgNum := lastMsgReadConf+1`, `express.e:11984-11985`; `lastMsgReadConf := cb.confYM`, `:4912`) — the sequential read pointer, **not** `scan_mail::first_unread_number_for`. On an exhausted/empty base the range collapses to `QUIT`: `R\r\n\r\n\r\n\x1b[32mMsg. Options: …\x1b[32m(\x1b[0m QUIT\x1b[32m )\x1b[0m>: ` and a `<CR>` / `Q` returns to the menu (`amiexpress_sysop.txt:357-365`; loop entry `express.e:11984-11985, 12008-12012`).
- The old one-line `Usage: R <message-number>` rejection (`READ_REQUIRES_NUMBER_LINE`) is gone for bare `R`.

**Deferred (B9):** at the `QUIT`-from-start prompt (no current message) Rust hides the `D`/`M` columns, since it gates them per-message; legacy gates them on the per-user `checkSecurity` ACS flags and would still show them. Faithful per-user ACS gating is slice B9.

### R `<num>` out of range / not found — BEHAVIOURAL

- Rust: `R 99\r\n\r\nMessage not found.\r\n` (`MESSAGE_NOT_FOUND_LINE`) for any number with no message.
- AmiExpress distinguishes two cases (live `amiexpress_read_messages.txt`): reading **past the last message** emits `\r\nThe last message in this conference is <high>\r\n` — the live `R 2` (only msg 1 exists) returned `The last message in this conference is 1` and bounced to the menu; a **gap in the middle** (a deleted/missing file between valid messages) is silent, the read loop simply iterates past it.

So AmiExpress is *not* uniformly silent on a bad number (a live-data correction to the earlier source-derived parity note, which described only the silent mid-gap case): the out-of-range case is a distinct notice. Rust collapses both to a single `Message not found.`. **BEHAVIOURAL** — different control flow and a different (Rust-only-uniform) notice. (This applies only to an explicit `R <num>`; slice B10 fixed the bare-`R` exhausted path, which now shows the `( QUIT )` prompt and returns silently rather than leaking `Message not found.`.)

### R `<num>` deleted — MATCH

Both emit the deleted notice. Rust `DELETED_MESSAGE_LINE` = `\r\nThat message has been deleted.\r\n\r\n` equals legacy `[..]That message has been deleted.\b\n\b\n` (`express.e:8890`).

### Sub-prompt option string + range — MATCH

- Rust: `\x1b[32mMsg. Options: \x1b[33mA\x1b[36m[,\x1b[33mD][,\x1b[33mM]\x1b[36m,\x1b[33mF\x1b[36m,\x1b[33mR\x1b[36m,\x1b[33mL\x1b[36m,\x1b[33mQ\x1b[36m,\x1b[33m?\x1b[36m,\x1b[33m??\x1b[36m,\x1b[32m<\x1b[33mCR\x1b[32m> \x1b[32m(\x1b[0m 1+4 \x1b[32m )\x1b[0m>: ` (`render_read_subprompt`, `wire_text.rs:491`).
- Legacy: assembled at `express.e:12016-12021`, including the doubled-`[36m` seam when `D`/`M` are suppressed and the `<msgNum>+<highMsgNum-1>` range (`:12010`).

`D` is gated on delete-access (`ACS_DELETE_MESSAGE`, `:12017`) and `M` on sysop-read (`ACS_SYSOP_READ`, `:12018`) in both. The AE live prompt shows `A,D,F,R,L,Q,…` (no `M`) because that test sysop lacks `ACS_SYSOP_READ`; Rust shows `A,D,M,…` because its sysop has it. Same gating, different seeded access — not an implementation difference (the per-user vs per-message gating nuance is slice B9).

> **Range numbering (slice B10):** the range lower bound is the **next** message to read (the legacy increments `msgNum` *after* `displayMessage`, `:12372`), and it collapses to the literal `QUIT` when that pointer passes the last message (`:12012`). The pre-B10 Rust captures (e.g. `1+4` in `rust_sysop.txt`) showed the *just-displayed* number — an off-by-one since corrected, so a fresh `R 1` now opens at `2+<high>` and the last message's prompt reads `( QUIT )`, matching legacy.

### Sub-prompt `<CR>` / `A`gain / `L`ist / `Q`uit — MATCH

- `<CR>` advances to the next message (legacy `goNextMsg`, `:12082`); Rust live CR moved msg 1 → msg 2.
- `A`gain re-renders the current message and re-prompts.
- `L`ist: Rust `\x1b[32mStarting message \x1b[33m[\x1b[0m1\x1b[33m]\x1b[0m: ` == legacy `:8831`; empty input defaults to lowest-not-deleted; the `Msg    Type     From … Subject` header + `------ …` rule + `Public `/`Private` rows match `:8857-8864`.
- `Q`uit returns to the menu.

### Sub-prompt `?` / `??` help — MATCH (implemented subset)

- Short (`?`): `A>gain / D>elete Message / M>ove / F>orward / R>eply / L>ist / Q>uit / <CR>=Next ( <range> )?` reproduces `express.e:12024-12031` verbatim.
- Long (`??`): adds `M>ove Message`, `L>ist all messages`, `EH> Edit Message Header` (`:12035-12059`).
- **COSMETIC (intentional subset):** the long list omits the legacy `NS`, `T/TS/T!/T*`, `K`, `E`, `EM`, `U` entries (`:12041-12056`), documented as out-of-scope in slice B5. Every shared entry matches byte-for-byte.

---

## MS — Mail Scan (multi-conference)

**Command identity.** `MS` is the *same* command in both systems — legacy `internalCommandMS` (`amiexpress/express.e:25250`, dispatched at `:28350`) and Rust `MenuCommand::ScanAllMail`. It is not a renamed or relocated command. (Bare `M` is the ANSI toggle in both; AE also has a distinct `ZOOM` = gather-mail command at `:28390`, unrelated to `MS`.)

**Behaviour summary.** `MS` walks every accessible conference. AE's `internalCommandMS` calls `joinConf(..,FORCE_MAILSCAN_ALL)` over every conf/msgbase; Rust's `scan_all_mail` walks the same set by coordinate. Per base, the legacy `searchNewMail` `currentConf=0` branch (the MS path) prints a `Type/From/Subject/Msg` column table when there is matching unread mail, or `No mail today!` when there is none. Rust reproduces this exactly.

**Wire-format comparison (matches).**

- Header: AE `'\b\nScanning conferences for mail...\b\n\b\n'` (`:25258`) vs Rust `b"\r\nScanning conferences for mail...\r\n\r\n"` — identical text.
- Conference banner: both emit `\x1b[32mScanning Conference\x1b[33m: \x1b[0m<name> - ` (`:11670`; `render_scan_conference_banner`). Green label / yellow colon / reset all match.
- Message-base sub-line: both emit ` \x1b[32mMessage Base\x1b[33m: \x1b[0m<base> - `, with a leading CRLF only for the first base (`IF msgBaseNum=1 THEN \b\n`, `:11676`; `render_scan_msgbase_banner` `first_base`).
- `No mail today!`: identical wording and identical branch (`:11689`; `MAIL_SCAN_NO_MAIL_TODAY`).
- Listing table: identical — two leading CRLFs, green `Type     From                           Subject                Msg    `, yellow `-------  -----------------------------  ---------------------  -------`, a standalone `\x1b[0m`, then `status(7)  from(29)  subject(21)  \x1b[0m<6-digit msg>` rows (`:11713-11720`; `render_scan_listing_table`). Status is `Public ` for public mail, `Private` otherwise (`:11719`; `scan_row_status`).

In the live transcripts both correctly show `Scanning Conference: <name> - ` / `Message Base: <base> - No mail today!` for empty bases; only the seed values differ (Rust conf 1 lists Main + Programming, AE conf 2 lists New Users + Amiga), which is data, not behaviour.

**Differences.**

- **COSMETIC — line endings.** AE source uses Amiga `\b\n`; Rust emits telnet `\r\n`. Universal translation, not MS-specific.
- **BEHAVIOURAL — `Found Mail!` line absent in Rust.** AE's single-conf/auto-join path prints `\b\nFound Mail!` (`:11736`); Rust emits this string nowhere.
- **BEHAVIOURAL — no pagination during the scan.** AE pages the scan: `lineCount:=2` (`:25257`) and `checkForPause()` after each listing row (`:11722`), shown stopping at `(Pause)...Space To Resume: ` in `amiexpress_login.txt`. Rust builds the whole output and flushes once — no `checkForPause`, no `lineCount`, so the scan never pauses regardless of length.

**Verdict:** the scan output matches byte-for-byte and the read-it-now prompt + drop-into-read matches; the residual gap is the mid-scan `checkForPause()` pagination. (The `Found Mail!` line belongs to AE's single-conf/auto-join path, gated on `currentConf<>0`, which `MS` never hits — a non-issue for MS parity.)

---

## E (Enter Message) and C (Comment to Sysop)

Both systems implement `E` and `C` as line-mode mail composition reached from the main menu: a `To` -> `Subject` -> (`Private`) -> body sequence for `E`, and a sysop-addressed `Subject` -> body sequence for `C`. The composition *behaviour* is broadly equivalent — recipient resolution, unknown-user rejection, conference-access gating, blank-subject abort, default-N private flag — but the rendered prompts and a few control-flow signals diverge. The comparison-test battery deliberately aborts before the editor body on the AmiExpress side, so the editor/save path is only exercised live on the Rust side.

### E — To: prompt

AmiExpress paints a boxed header and an ANSI-coloured `To` prompt with an `(Enter)='ALL'?` default hint, via `msgToHeader()` (`express.e:9999-10000`). It does this for a bare `E`, for an inline `E <to>` (echoing the typed name), and inside `C`. Rust emits a bare `\r\nTo: ` only for a prompted bare `E`, with no box, no ANSI, and no name echo.

- Rust REPR: `b'\r\nTo: '`
- AE REPR: `b"...\x1b[32m(\x1b[33m------------------------------\x1b[32m)\x1b[0m\r\n     \x1b[36mTo\x1b[33m: \x1b[32m(\x1b[33mEnter\x1b[32m)\x1b[0m=\x1b[32m'\x1b[33mALL\x1b[32m'\x1b[32m?\x1b[0m "`

The blank-line `->` `ALL` default is equivalent on both sides, so the prompt decoration/ANSI/box is **COSMETIC**. However, the fact that Rust skips the To-echo/box for inline `E <to>` (AE always shows `To: ... SYSOP` per `express.e:10769-10771`, live `amiexpress_sysop.txt:329-333`) is **BEHAVIOURAL** — the user never gets a visible confirmation of the resolved recipient.

### E / C — Subject: prompt

- Rust REPR: `b'Subject: '`
- AE REPR: `b"\x1b[36mSubject\x1b[33m: \x1b[32m(\x1b[33mBlank\x1b[32m)\x1b[0m=\x1b[33mabort\x1b[32m?\x1b[0m "` (`express.e:10847` / `8783`)

The ANSI colour and `(Blank)=abort?` hint are **COSMETIC**. The abort *outcome* differs: AE aborts a blank subject silently (bare `\b\n`, `express.e:10855-10856` / `8788-8789`), while Rust writes `\r\nMessage aborted.\r\n` (`POST_ABORTED_LINE`). That silent-vs-notice difference is **BEHAVIOURAL**. (Neither live transcript actually submits a blank subject — AE stops at the prompt, Rust submits a real subject — so this rests on source, not live wire.)

### E — Private (y/N)? prompt

- Rust REPR: `b'Private (y/N)? '`
- AE: `'         \x1b[36mPrivate '` then `yesNo(2)` renders `\x1b[32m(\x1b[33my\x1b[32m/\x1b[33mN\x1b[32m)\x1b[32m?\x1b[0m ` (`express.e:10861-10862`, `2134`)

Both default to **N** (AE `yesNo(2)` maps CR->`n` at `express.e:2145`; Rust treats only `y`/`Y` as private). The ANSI colour, and the input mechanism (AE single-keystroke `readChar` echoing `Yes`/`No` vs Rust full-line read), are **COSMETIC** — same question, same default, same effect. `C` never shows a Private prompt on either side (the sysop comment is fixed private/`R`).

### E — unknown recipient

- Rust REPR: `b'\r\nUnknown user.\r\n'` (`POST_UNKNOWN_USER_LINE`)
- AE REPR (live `amiexpress_sysop.txt:319`): `b'User does not exist!!\r\n'` (`express.e:10814`)

Same behaviour (reject, return to menu, no compose). Wording/bang-count differ — **COSMETIC**.

### E — recipient lacks conference access

Not exercised live (the battery only drove the unknown-user path); source-derived.

- AmiExpress: `\b\nUser does not have access to this conference!\b\n\b\n` (`express.e:10838`).
- NextExpress: `\r\nUser does not have access to this conference.\r\n` (`POST_RECIPIENT_NO_ACCESS_LINE`).

Same behaviour (reject the addressee). **COSMETIC** — AE ends the sentence with `!` and a trailing blank line; Rust uses `.` and no trailing blank.

### E — addressing not allowed (EALL in an external base)

Not exercised live; source-derived.

- AmiExpress: `\b\nCan't use EALL in external message bases!!\b\n\b\n` (`express.e:10806`) — a per-addressing-kind notice.
- NextExpress: `\r\nThis message base does not accept that addressee.\r\n` (`POST_ADDRESSING_NOT_ALLOWED_LINE`) — one generic notice for all disallowed addressees.

**BEHAVIOURAL (text granularity):** AE emits distinct notices per addressing kind (EALL / ALL); Rust collapses them into a single generic line.

### C — recipient display

AE's `commentToSYSOP` prints the decoration box plus a pre-filled `To: (Enter)='ALL'? SYSOP` line before the Subject prompt (`express.e:8779-8783`, live `amiexpress_sysop.txt:343-347`). Rust's `handle_comment_to_sysop` goes straight to `Subject: ` (`post_mail.rs:116-125`) — the user is never shown that the comment is addressed to the sysop. **BEHAVIOURAL** (a visible step present in one, absent in the other).

### E — body editor and save (LIVE on both sides)

The driver drove a real post all the way through on AmiExpress (`amiexpress_post_and_list.txt`). `E`/`C` use the ruler / numbered-line editor with the `Msg. Options:` save menu, structurally aligned with the legacy. The residual differences are the dropped full-screen fork, the `F`/`X` (file-attach) menu verbs, and the save-confirmation wording.

AmiExpress first offers its **full-screen editor** and, on decline, drops into a **line editor** with a ruler and line numbers, ending in a save menu:
```
         Private (y/N)? Yes
FullScreen Editor (y/N)? No

   Enter your text. (Enter) alone to end. (75 chars/line)
   (|-------|-------|-------|-------|-------|-------|-------|-------|-------|--)
1 > This is a PRIVATE message addressed to sysop.
2 > Posted live via the comparison harness to show
3 > how AmiExpress renders message bodies.
4 >

Msg. Options: A,C,D,E,F,L,S,X,? >:S

Saving...Message Number 1...done!
```
(The save menu `Msg. Options: A,C,D,E,F,L,S,X,?` — A>bort, C>ontinue, D>elete lines, E>dit, F>ile attach, L>ist, S>ave, X>fer, `express.e:10375-10389` — is a *different* `Msg. Options:` prompt from the read sub-prompt that shares the prefix.)

NextExpress drives this via `read_editor_body` (`menu_flow/post_mail.rs`): the ruler intro, `N > ` numbered prompts (blank line ends input), then the save menu — rendered for the no-file-attach case, so `F`/`X` are absent:
```
   Enter your text. (Enter) alone to end. (75 chars/line)
   (|-------|-------|-------|-------|-------|-------|-------|-------|-------|--)
1 > <text>
2 >

Msg. Options: A,C,D,E,L,S,? >:S

Message #N saved.
```
`S` saves, `A` aborts (with the `Abort message entry (y/n)?` confirm), `C` continues editing, `L` lists, `?` shows the verb help; `D`/`E` (delete/edit lines) are advertised but deferred. The `FullScreen Editor (y/N)?` fork (`yesNo(2)`, default N, `express.e:10099-10100`) is skipped entirely, per scope.

Residual differences: NextExpress drops the full-screen fork and the `F`/`X` file-attach verbs (no file-attach feature), and keeps its own `Message #<n> saved.` confirmation rather than AE's `Saving...Message Number <n>...done!`. The body cap reuses the same `append_line_with_newline` helper as before. Reply / forward keep the minimal `.`/`/A` editor (`POST_BODY_PROMPT`) — they are not the "message entry" path.

### Notes / corrections vs the earlier source-derived parity table

The live transcripts corroborate the earlier source-derived notes for the `E`/`C` family: plain Rust prompts vs ANSI-decorated AE prompts, `Unknown user.` vs `User does not exist!!`, and the missing To/decoration echo for inline `E <to>` and for `C`. The private-flag default is confirmed **N** on both sides (not the Y/N mismatch an early reading suggested) — against `yesNo(2)` and the Rust matcher. The AE menu prompt is labelled `NextExpress Reference` — this is the reference AmiExpress build used as the parity oracle, not a separate system.

---

## Unknown Command, Retired RP/FW/K/MV/EH, and N

Logged in as sysop on both sides (Rust conf 1 "Main", AE conf 2 "Amiga"). The probes were `RP 1`, `FW 1`, `K 1`, `MV 1`, `EH 1`, `N`, and a genuinely-bogus `FOObar`/`FOOBAR`.

**IMPORTANT correction to the earlier source-derived parity note.** It claimed AmiExpress is *silent* on unknown commands ("no `Unknown command.` string in express.e"). The live AE transcript and the source both refute this: AE emits a notice. The real difference here is the *string*, not silent-vs-notice.

### Unknown command (`FOObar` / `FOOBAR`)

Both reject and re-prompt; the session continues. The notice strings differ:

- Rust: `b"FOObar\r\nUnknown command. Type G to log off.\r\n..."` (`UNKNOWN_COMMAND_LINE`, `wire_text.rs:203`), notice on the line immediately after the echo.
- AE: `b"FOOBAR\r\n\r\nNo such command!!  Use '?' for command list.\r\n\r\n\r\n..."` (`express.e:28397`: `aePuts('\b\nNo such command!!  Use ''?'' for command list.\b\n\b\n')`), wrapped in blank lines.

**COSMETIC** — different wording and blank-line framing; both notify and re-prompt.

After the notice, **Rust redraws the entire ASCII-art menu** (banner + all sections, ~30 lines) before the prompt (`menu_flow/mod.rs:80-83`, non-expert), whereas **AE prints only the notice + the one-line prompt** — no full menu re-draw. **COSMETIC** for this group (it is the same redraw cadence for every command, not specific to unknown), but it sharply changes screen volume.

AE additionally runs `IF res=RESULT_NOT_ALLOWED AND privcmd=FALSE THEN higherAccess()` on the fall-through (`express.e:28400`); Rust's `MenuCommand::Unknown` arm has no such hook. Not observable for a plain bogus token but a **BEHAVIOURAL** branch present only in AE.

### `N`

**BEHAVIOURAL.** AE: `N` is a real command — `internalCommandN` (`express.e:28352 → :25275`) gates on `ACS_FILE_LISTINGS` and tail-calls `myNewFiles` (the new-**files** scan / AquaScan). The AE menu advertises `[N]......nEW FILES SCAN` and the transcript drops into the interactive `--[ AquaScan v1.0 ... ]` date/directory prompts. Rust: `parse_menu_command("N") == MenuCommand::Unknown` (`menu_command.rs:260`), so `N` yields the unknown-command notice. This is the intentional Tier B B2 removal of the old `N`→mail-scan binding; the legacy `N`=new-files scan is deferred to Tier D.

### `RP 1`, `FW 1`, `K 1`, `MV 1`, `EH 1` (retired top-level forms)

Both sides reject all five at the top level — but for different reasons and with one Rust inconsistency:

- AE never had them as top-level commands; `R>eply`, `F>orward`, `D>elete`, `M>ove`, and `EH` live inside the `R`/`readMSG` sub-prompt. AE's menu does not list them, so AE's menu and dispatcher agree.
- Rust retired them from the dispatcher in Tier B B8, so `RP 1`/`FW 1`/`K 1`/`MV 1`/`EH 1` all hit `MenuCommand::Unknown` → `Unknown command. Type G to log off.`

**BEHAVIOURAL (Rust internal inconsistency).** The Rust main menu *used to* advertise all five as top-level commands — the transcript showed `RP <n>   Reply to message number <n>`, `FW <n>   Forward message number <n>` in MESSAGES and `K <n>   Kill/delete message number <n>`, `MV <n>   Move message number <n>`, `EH <n>   Edit message header for number <n>` in MAIL ADMIN — yet every one was rejected as unknown. The menu text was not updated when the commands were retired. AE has no equivalent menu/dispatcher mismatch.

**Resolved 2026-06-15.** `Conf02/Menu5.txt` no longer lists `RP`/`FW`/`K`/`MV`/`EH`; the two MESSAGES rows were dropped and the now-empty MAIL ADMIN section removed entirely. The guard test `main_menu_advertises_exactly_the_implemented_commands` was the reason this lingered — it filtered the menu's tokens down to the ones that still parse *before* diffing, so the retired tokens were invisible to it. It now reads back every four-space-indented command row verbatim, so any future advertise-then-reject entry fails the test.

### Net

- Unknown-command handling: **COSMETIC** (both notify; AE's `No such command!!  Use '?' for command list.` vs Rust's `Unknown command. Type G to log off.`). The "AE is silent" claim in the brief/parity doc is wrong per live data.
- `N`: **BEHAVIOURAL** (real new-files scan in AE; Unknown in Rust, deferred to Tier D).
- `RP/FW/K/MV/EH`: both reject. Rust formerly **advertised-then-rejected** — a **BEHAVIOURAL** internal inconsistency absent in AE, **resolved 2026-06-15** by dropping the stale menu rows so the menu and dispatcher agree.

---

## G — Goodbye / Log Off

Logs the user out of the BBS from the main menu and drops the connection.

### G (immediate logoff)

**Behaviour.** Both implementations echo the typed command, optionally display a sysop-supplied logoff splash screen, then terminate the session and close the connection. Beyond that the sequences diverge.

**Wire format (Rust, sysop, `comparison/transcripts/rust_sysop.txt:1701`):**
```
b'G\r\nGoodbye!\r\n'
```
After echoing `G\r\n`, Rust looks up the optional logoff screen (`menu_flow/mod.rs:134`, `Screens/LOGOFF.txt`). On a fresh install the asset is absent and `FALLBACK_LOGOFF = b""` (`file_screen_repository.rs:66`), so it is skipped. Rust then writes the literal `Goodbye!\r\n` (`wire_text.rs:206`, `GOODBYE_LINE`) and closes.

**Wire format (AmiExpress, sysop, `comparison/transcripts/amiexpress_sysop.txt:408`, `comparison/transcripts/amiexpress_login.txt:111`):**
```
G Y\r\n
\r\n
** AutoSaving File Flags **\r\n
<BEL>\r\n
Click...<ESC[0m>
```
AE emits **no** "Goodbye!" text. After the optional `SCREEN_LOGOFF` splash (gated at `express.e:8187`), `internalCommandG` calls `saveFlagged()` (`express.e:25064`), which prints `'\b\n** AutoSaving File Flags **\b\n'` (rendered `\r\n…\r\n`) and rings the bell via `sendBELL()` (`express.e:2803-2804`). It then prints `Click...` (`express.e:8191`) and drops carrier.

- **COSMETIC** — Closing wording. Rust now emits `** AutoSaving File Flags **` + `<BEL>` on every `G` logoff (slice D5-banner, matching AE's banner — including the nothing-flagged case, live-confirmed in `comparison/transcripts/ae_tierd_g_empty.txt`); the residual difference is the final word only — Rust's `Goodbye!\r\n` vs AE's `Click...`. Both merely mark end-of-session ahead of the identical carrier drop. (The older `rust_sysop.txt` battery capture predates D5-banner, so it still shows the bare `Goodbye!`.)
- **COSMETIC** — AE rings the bell (`<BEL>`) on every logoff; Rust now rings it (inside the autosave banner) only when leaving with files flagged. Audible only.
- **COSMETIC** — AE prints a `Click...` teardown notice before dropping DTR; Rust prints nothing equivalent.

### G vs `G Y` — the flagged-file confirm (BEHAVIOURAL)

The battery forces logoff with `G Y`, and that argument matters.

In AE, `internalCommandG` (`express.e:25047`) sets `auto:=paramsContains('Y')` (`25053`). When `auto=FALSE` (plain `G`) it calls `checkFlagged()` (`25058`):
- if files are flagged, the user is **prompted** (`yesNo(2)`, default N, `:12670`/`:2129`); answering **N** yields `mystat=0`, so it prints `\b\n` and **returns to the menu** (`25059-25062`); answering **Y** falls through to logoff;
- if nothing is flagged, `checkFlagged` returns `1` (`ENDPROC 1`, `:12674`), so `mystat≠0` and it **logs off** — *not* a menu return.

Only `G Y` (or a `Y` answer to the confirm, or an empty flag set) reaches the unconditional `saveFlagged()` / `setEnvStat(ENV_LOGOFF)` path.

**Resolved (slice D5/Ga).** Rust's `MenuCommand::Logoff { auto }` (`menu_flow/mod.rs`) now models this: plain `G` with a non-empty session flag set runs `confirm_leave_flagged()` (the `checkFlagged` prompt + single-key `yesNo` echo), returning to the menu on `N`/default; `G Y`, a `Y` answer, or an empty flag set log off. The confirm wire is byte-pinned to the live capture `comparison/transcripts/ae_tierd_g_confirm.txt`.

- **MATCH (slice D5/Ga)** — plain `G`'s confirm, the single-key `Yes`/`No` echo, the `N`→menu-return and the nothing-flagged→logoff control flow now match AE byte-for-byte.
- **MATCH (slice D5-banner)** — every `G` logoff now emits `saveFlagged()`'s visible `** AutoSaving File Flags **` banner + `<BEL>` before the goodbye, unconditionally — the banner precedes saveFlagged's own flag-count gate (`express.e:2803`), so it shows even with nothing flagged. Byte-pinned to `comparison/transcripts/ae_tierd_g_confirm.txt:177` (flagged) and `comparison/transcripts/ae_tierd_g_empty.txt` (empty). Only the Stay branch (plain `G` + flagged + `N`) skips it.
- **RESOLVED (slice D5-persist)** — the per-slot flag set is now saved on logoff and restored on logon via the `FlaggedStore` port (`domain/files/flagged_store.rs`): `InMemoryFlaggedStore` (default, process-lifetime) or `SqliteFlaggedStore` (durable across restarts), selected by `config.user_storage`. Keying is `(conference, name)` — since the July 2026 identity fix the domain `FlaggedKey` itself carries no area (matching the legacy `isInFlaggedList` identity, `express.e:12534`), so a restored flag appears in the logon banner, the `A` listing, and paints the `[X]` marker on the next `F`/`R` scan. The logon `** Flagged File(s) Exist **` banner (`\r\n** Flagged File(s) Exist **\r\n\x07\r\n`, live-captured in `ae_tierd_alterflags.txt`) is emitted by `restore_flags_and_announce` when the restored set is non-empty — position: after `render_login_stats`, before the menu loop, matching the capture. `saveHistory()` and the `dump` partial-downloads file remain deferred to the file-transfer slice.

### Note on the sysop gate

In AE, both `SCREEN_LOGOFF` and the `Click...` line are guarded by `logonType<>LOGON_TYPE_SYSOP` (`express.e:8187`, `8191`). The live `amiexpress_sysop.txt` transcript nonetheless shows `Click...`, so the observed teardown notice is taken from the live wire (authoritative) rather than the source gate. Rust gates its logoff splash on asset presence (empty fallback) rather than on logon type; the practical result on a fresh install — no splash shown — matches.

---

## F — File Listings (NextScan vs the AquaScan door)

**Parity target.** The stock deployment shadows `F`/`FR`/`N` with
AquaScan v1.0 door icons (`processCommand` runs BBSCmd icons before
internal commands, `express.e:28229-28256`), so the reference
experience for `F` is the door's, and that is what NextExpress
implements — the **NextScan** lister (user decision 2026-06-10;
ground truth `comparison/evidence-tierD/live-observations.md`,
cleanest transcript `comparison/transcripts/ae_tierd_aquascan3.txt`).
The internal `internalCommandF` (`express.e:24877`, raw DIR-file
streaming with LF-CR line endings and the `(Pause)...(f)lags,
More(Y/n/ns)? ` pager) is recorded in the evidence doc as the stock
diff and is not implemented. Live captures win over source-derived
expectations throughout.

**MATCH (byte-for-byte against the captures).** The entry preamble +
banner frame shape; `Scanning dir N from top... Ok!/Nothing found!`
and the HOLD variant; the date-group separator art (Latin-1 bytes);
`[ File #N ]` headers incl. the 2-digit pad shrink; colour-framed
rows over the upload-writer column layout (check byte at col 13,
RJ-7 size, `MM-DD-YY`, col-33 continuations); plain fall-through for
unframeable rows (13-char/over-long names, 8-digit sizes);
`[ End of File List ]` + the unconditional post-End `More?`; the
`More?` verb set — `Y`/unknown resume via the 69-space overprint
clear, `C` form-feed, `Q` → `Quit`, lone `n` held as the ambiguous
`N`/`ns` prefix and erased `\x08 \x08` by the next verb, `ns` →
`Non-stop scrolling! Are you sure (Y/n)? ` (decline redraws),
`F`/`R` → the two distinct silent flag prompts with the 79-space
clear, `?` → the in-pager pause help; `F A` per-dir
footer/More?/transition choreography incl. back-to-back headers over
empty dirs; bare `F` → `Directories: (1-N), …(Enter)=None ? ` with
silent Enter-abort and `Error in input!`; `F 99` →
`The highest directory number is N!`; junk args → the help banner +
`Argument error! Type 'f ?' for help.`; the `F ?` help screen; the
per-path exit-tail asymmetry (two resets for listing exits, one for
aborts/argument errors). **Interaction (slice D2b):** the
`More?`/ns-confirm/flag reads act on single keypresses — true hotkeys
via `Terminal::read_key`, no Enter — so NextScan matches the door's
*interaction* as well as its bytes. Two corners are probe-pinned:
held-`n` + Enter quits with the bare CR echoed as `\r\n` + the exit
tail, no `Quit` word, no `BS SP BS` (probe P1,
`comparison/transcripts/ae_tierd_probes.txt:100-138`); a bare LF at
`More?` is swallowed and reaches no verb (probe P2, `:140-175`). Verb
case-insensitivity (`Q`/`Y` upper, `n`/`ns` lower captured; mixed case
folded both ways) is **INFERENCE** — only those cases were captured.

**COSMETIC (deliberate, documented).**

| Divergence | Detail |
|---|---|
| NextScan branding | Three swaps, frame widths held by stretched dash runs: banner centre `NextScan ` (40/34 dashes), `Copyright © 2026 NextScan `, `- Configure NextScan` (`designs/NEXTSCAN.md` §7) |
| Page positions ≥ page 3 | NextScan pages at a flat 29 lines (matches captured pages 1–2 exactly); the door's own counter drifts from page 3 |
| `?` redraw window | NextScan redraws exactly the current page's lines; the door redraws a drifted window of its internal page memory |

### FR — reverse listing (slice D3)

**Parity target.** Same AquaScan door, reverse mode. `internalCommandFR`
(`express.e:24883`) → `displayFileList(params, TRUE)` is the shadowed
stock path (diff record only); the wire is AquaScan's.

**MATCH (byte-for-byte against the captures).** Banner right label
`'fr ?' for options` with the dash run flexed 40→39 to hold the 77-col
frame; `Reverse-scanning dir N... Ok! / Nothing found!` header (no
"from top"); per-dir rows emitted newest-first (the area's rows
reversed) — `ae_tierd_aquascan3.txt` S10 `FR 1`. All
frame/colour/pager/footer/exit-tail behaviour is the D2 `F` code path
unchanged.

**BEHAVIOURAL (source-derived; departs from the AquaScan capture).**
Two places follow `express.e` over the captures:
- **Bare `FR` opens the `Directories:` prompt** (under the reverse
  banner), like bare `F`, then reverse-walks the chosen span —
  `displayFileList` branches only on params-present vs. bare, and bare
  → `getDirSpan('')` shows the prompt regardless of `reverse`
  (`express.e:27643-27648`). The AquaScan **capture** (S11) instead
  shows bare `FR` *skipping* the prompt and scanning the highest dir;
  we override that with the original behaviour per the "use the original
  code" rule (amended 2026-06-18, reversing the initial decision). The
  bare-`FR`-prompt bytes are therefore extrapolated, not captured. Bare
  `F` and bare `FR` are now **symmetric** (both prompt).
- **`FR A` descends the multi-dir span highest→lowest** —
  `displayFileList`'s reverse loop walks `dirScan→startDir` (`fLLoop--`,
  `express.e:27654`), each dir's rows reversed. The captures only
  exercise single-dir `FR`, so the multi-dir order is `express.e`-derived.

**UNVERIFIED.** `FR ?` (reuses the `F ?` help — no distinct `'fr ?'`
help screen captured); `FR H` reverse hold (uncaptured; the hold header
stays the forward `Scanning HOLD dir from top...`, matching
`displayFileList :27688` which emits it unconditionally).

**BEHAVIOURAL (deliberate, documented).**

| Divergence | Detail |
|---|---|
| Art/© byte encoding | NextScan emits UTF-8 multi-byte sequences for high-bit glyphs (art `\xb8\xf8\xa4…` → `\u{b8}\u{f8}\u{a4}…`, © `\xa9` → `\u{a9}`); the AquaScan door emitted raw Latin-1 single bytes. Deliberate policy — see AGENTS.md "Wire encoding"; design rationale in `designs/2026-06-12-utf8-hotkeys-flagmark-design.md`. |
| On-row flag marker (slice D2f) | `F`/`R` flag listed files into a session set; an aligned row gains a 4-column `[X] ` marker slot between the name and the check byte, an over-long row a trailing ` [X]` — a deliberate NextExpress aid the AquaScan door has not (its rows stay byte-identical to the captures). The captured F/R prompt exchange is unchanged (flagging is silent there); flagging a row still on the current page additionally repaints its marker in place (cursor up, `\x1b[14G[X]`, cursor back), suppressed when ANSI is off. Design §5; `designs/2026-06-12-utf8-hotkeys-flagmark-design.md`. The logoff `checkFlagged` warning (slice Ga), the `** AutoSaving File Flags **` autosave banner (slice D5-banner), the `A` alter-flags verb — the read-only listing (D6a) plus the `flagFiles` add/clear prompt loop (D6b) — and the cross-session flag persistence + the logon `** Flagged File(s) Exist **` banner (slice D5-persist) have all landed. Note: the flag identity is `(conference, name)` with no area (July 2026 fix, matching the legacy `isInFlaggedList`, `express.e:12534`), so a flag restored from a previous session — or set by name at the `A` prompt — paints the `[X]` marker on the next `F`/`R` scan of any dir listing that file. |

**UNVERIFIED (provisional, tagged in test names).** `F 0` →
highest-dir error; unknown `More?` keys continue; the counter reset
at dir transitions; zero-area conferences; the `H` prompt option for
non-hold users; framed rendering of real held files (unit-pinned
only); whether the door accepts the `Q`-token/`F W` forms rather than
Argument-erroring (`F R` with a space is now settled — D3 ships the
concatenated `FR` reverse token and keeps `F R` on the Argument-error
path, `express.e:28310`). The
help-advertised navigation verbs (`3`/`9`/arrows/`7`/`5`/`K`/`L`)
and cross-tier verbs (`D`/`X`/`V`/`O`/`Z`/`A`) are
advertised-but-inert — unknown keys continue, the door's own
default — each owed to its owning slice.

---

## Z — Zippy Text Search (genuine internal command, slice D4)

**Parity target.** Unlike `F`/`FR`/`N`, `Z` is **not** in the AquaScan
door icon set (`CS, F, FR, N, NSU, SCAN, SENT`), so typing `Z` on the
stock board runs the genuine `internalCommandZ` (`express.e:26123`), not
a door. The parity target is therefore the internal command, captured
live in [`comparison/transcripts/ae_tierd_zippy.txt`](comparison/transcripts/ae_tierd_zippy.txt)
(Z1–Z7, search-string/dir prompts, single-dir / `A` / no-match / blank
aborts, case-insensitive) and [`ae_tierd_zippy2.txt`](comparison/transcripts/ae_tierd_zippy2.txt)
(ZU upload, ZH hold, ZOOR out-of-range). NextExpress reproduces the
internal wire: **plain** raw-DIR-row dump (no NextScan frames or
colour — deliberately unlike the `F` door), the `Enter string to search
for: ` prompt, the internal `getDirSpan('')` `Directories: …=none? `
prompt, `Scanning directory N` headers, and the `No such directory.`
error.

| Aspect | NextExpress (Rust) | AmiExpress (internal `Z`) | Tag |
|---|---|---|---|
| Command resolution | Exact-token `Z`; `ZOOM` stays separate | `StrCmp(cmdcode,'Z')` (`:28388`) | MATCH |
| Search string | `item(0)` (first token); bare `Z` → `Enter string to search for: ` prompt; empty answer returns | `:26146` / `:26150-26156` | MATCH |
| Directory prompt | Internal `getDirSpan('')` `Directories: (1-N), (A)ll, (U)pload, (H)old, (Enter)=none? ` — lowercase `none`, space after `?`, reset with **no** trailing space | `:26864` | MATCH (distinct from the AquaScan `F` `=None ?` prompt) |
| Dir answers | number → single dir; `U` → highest dir (by number); `A` → all areas; `H` → hold (`Scanning directory HOLD`); blank → none-abort; out-of-range → `No such directory.` | `getDirSpan` `:26881-26908`, `:26905` | MATCH |
| Match rule | `UpperStr`+`InStr` over each rendered DIR row (filename row included); any matching line dumps the whole block, continuations included; case-insensitive | `zippy()` `:27529-27620`, `:27595-27598` | MATCH |
| Row rendering | Raw `dir_row` (name/check/size/date/desc), **no** frames/colour | `aePuts(current)` raw DIR lines | MATCH |
| Inline area-spec `Z <q> <span>` | `item(1)` resolved inline via the same `getDirSpan` logic — `Z ART 1`/`A`/`U`/`H` scan **immediately, no prompt**; out-of-range/junk → `No such directory.` (slice D7) | `getDirSpan(item(1))` `:26162-26163` | MATCH (captured `ae_tierd_zippy3.txt`) |
| Large-match pagination | Emits without pausing | `flagPause` per line (`:27582`/`:27613`) | BEHAVIOURAL (uncaptured; deferred) |

The pre-prompt trailing differs only by NextExpress's own menu
convention (the handler owns internalCommandZ's bytes; the menu loop
re-renders the menu screen rather than the legacy single lead-in blank) —
a slice-independent, pre-existing presentation difference shared by every
command, not specific to `Z`.

---

## N — new-files scan (AquaScan door, slice D9)

**Parity target.** Like `F`/`FR`, `N` is AquaScan-shadowed on the
stock board (`BBS:Commands/BBSCmd/` icons dispatch before internals,
`express.e:28229-28256`), so the reference experience is the door's
date-scan mode, captured live in the dedicated two-pass transcript
[`comparison/transcripts/ae_tierd_newfiles.txt`](comparison/transcripts/ae_tierd_newfiles.txt)
(sections N1–N9, ~30 probes; harness
`comparison/harness/ae_tierd_newfiles.py`). The shadowed
`internalCommandN` (`express.e:25275`, the *looping*
`Date as (mm-dd-yy) to search from (Enter)=: ` prompt) is the stock
diff record only. Design brief:
`designs/2026-07-03-n-newfiles-scan-design.md`.

**MATCH (byte-for-byte against the captures, branding aside).** The
entry preamble (reset line, banner, blank); the
`Date: (MM-DD-YY), (-X) Days, (R)everse, (Enter)=<mm-dd-yy> ?` prompt
with the full captured SGR runs and trailing space; the Enter default =
**day of the previous call** (capture-proven: pass 1 advertised
`06-25-26` while today was 07-03); the door's `Directories:` prompt
byte-identical to bare `F`'s, **current conference only** (N9: `(1-1)`
in a one-area conference); `Scanning dir {n} for {mm-dd-yy}... Ok! /
Nothing found!` headers (plain, uncoloured);
`Scanning dir {n} for the last {x} files... ` for `!x`; the filtered
body renumbered from `[ File #1 ]` (N3: `-30` → PROTRACK is #1) with
the full F frame/separator/pager machinery (base `More?` — zero
`(S)kip Conf` anywhere); BADUPLD.LHA (check `F`, Available) **listed**
when in range (N2 File #19); `R` (and `R <date>` — date discarded,
N4b) runs exactly the FR full-reverse mode; empty dirs run
header-into-header; Enter=None aborts with F's single-reset tail;
`Error in date!` (N5) and out-of-range-dir (N8b) envelopes; the `N ?`
help screen skeleton (N6); inline grammar
`N [S|mm-dd[-yy]|T|Y|-x|!x|R] [dir] [Q] [NS]` (N7a–N7r: Upload-dir
default, `-30` month underflow → `06-03-26`, `T`/`Y`/`S`, bare digit =
dir, `!2` newest-pair ascending, `Q` drops description continuations,
`NS` suppresses every `More?`); `N R -1` → the Copyright banner +
`Argument error! Type 'n ?' for help.`; every listing-shaped exit =
the two-reset tail. Two page-1 models, both capture-pinned: the
prompt path counts 29 lines **from the post-answer blank** (the door
resets its counter at interactive prompts, N2); the inline path counts
29 **from the reset line** (F's span-path model, N7c).

**BEHAVIOURAL / COSMETIC (deliberate, documented).**

| Divergence | Detail |
|---|---|
| NextScan branding | The N banner centre label `AquaScan v1.0 by Aquarius/Outlaws ` (34 visible) → `NextScan ` (9), dash run 15→40, 77 visible cols preserved; right label `'n ?' for options ` is `'f ?'`-width so the landed F banner geometry holds. Help screen reuses the landed Copyright `NextScan` banner byte-for-byte and swaps `Configure AquaScan` → `Configure NextScan`. Width parity asserted against the kept AquaScan originals (`wire.rs` tests; `designs/NEXTSCAN.md` §7) |
| Single-shot `Error in date!` | The door errors once and exits to the menu (N5, captured) — NextExpress matches the door. The *internal* command's looping length-only prompt (`MiscFuncs.e:388-401` accepts any 8 chars) is diff-record only |
| Default date source | NextExpress derives the Enter default from `user.last_call()` (mutates only at session finalise). The internal models a separate `newSinceDate` bumped by a `newSinceFlag` at logoff (`express.e:27855`, `:27902`, `:8197`) — not modelled; the capture proves the door's default equals the previous-call day, which `last_call` reproduces. First-time caller (no prior call) → today (a NextExpress choice, TO-CONFIRM #12) |
| UTC day boundary | Cutoffs are UTC midnight of the target day, filter `uploaded_at >= cutoff` — **inclusive**, `express.e:27976-27986` `ddt>=day` (the `dir_row` UTC rendering precedent). The Amiga board's day boundary is its local clock |
| Uniform 29-line paging | Pages 2+ drift on the door (N2 segments 30/27/29/29/13); NextScan pages uniformly at 29 — the documented F COSMETIC divergence inherited |
| Per-file filtering | The internal dumps the rest of the DIR file after the first match (`express.e:27991-28013`); NextExpress filters per file. Equivalent under a chronologically-sorted repository (the upload writer appends in order) — unobservable on the wire |
| ACS gating | NextExpress gates neither `F` nor `N` (consistent); the internal gates `ACS_FILE_LISTINGS` (never the unused `ACS_NEW_FILES_SINCE`, `axcommon.e:12`) |
| `N W` not ported | The (rebranded) help screen advertises `N W - Configure NextScan`, but the door's self-configuration is not ported — `N W` takes the Argument-error envelope (the `F W` precedent; NextExpress config is TOML) |
| Art/© byte encoding | Same UTF-8 policy as `F` (AGENTS.md "Wire encoding") |
| Menu advert | `Conf02/Menu5.txt` gains an `N [date]` row — the `?` menu wire now differs from the shipped AquaScan board's menu (which advertises no N row) |

**PLAUSIBLE (uncaptured; shipped provisionally, each quarantined
behind its own const/test — one-line fix on re-probe).** Numbered
after the design brief's §1.2 TO-CONFIRM list:

| # | Surface | Shipped behaviour |
|---|---|---|
| 1 | `H` at N's `Directories:` prompt / inline `N <date> H` | Date-/newest-filtered **held** rows under the dir→HOLD header substitution (`Scanning HOLD dir for <label>...`). The prompt advertises `(H)old`, so a defined, non-panicking behaviour ships; header wording unprobed |
| 2 | `T`/`Y`/`S`/`!x` typed at the Date prompt | `Error in date!` (only date/`-x`/`R`/Enter/junk were probed there) |
| 3 | Junk at N's `Directories:` prompt | F's `Error in input!` envelope (same door machinery, byte-identical prompt) |
| 4 | Inline letter spans `N <date> A` / `U` | Resolved via the shared span-token resolver (inferred from the help diagram; only numeric dirs and the Upload default were captured) |
| 5 | Bare `N <dir>` date source | SinceLastCall per the help grammar `N [S] [dir]` (pass-2's last call = today, so the capture cannot distinguish it from Today); pinned by a diverging-clock test as a NextExpress choice |
| 6 | `N mm-dd` (year omitted) | Current year from the Clock port |
| 7 | Calendar-invalid but date-shaped input (`13-40-26`) | Rejected — `Error in date!` at the prompt / Argument error inline (the internal accepts any 8 chars) |
| 8 | `!x` edges (`!1` wording, `!x` > dir size, `!x` on empty dir, `Q`+`R` combos) | Header pluralisation unchanged (`the last 1 files`); overshoot saturates to the whole dir; empty dir → Nothing found |
| 9 | Pager verbs beyond `Y`/`Q` at an N `More?` | Engine-shared with F (same machine on the real door too), never exercised inside an N scan on the reference |
| 10 | Inline out-of-range dir (`N 9`) | F's highest-dir envelope (the prompt-path variant N8b was captured byte-identical) |
| 11 | Trailing junk after a valid date at the prompt | Rejected (`Error in date!`); `R <date>` shows extra tokens tolerated in that one captured form only |
| 12 | First-time caller default (`last_call == None`) | Today (not capturable — the reference sysop always has a prior call) |
| 13 | Date-prompt echo discipline (per-keystroke echo, backspace, the trailing-space final byte) | The AGENTS.md step-6 type-at-a-real-terminal item + the like-for-like FS-UAE pass own this |

The `specs/core.allium:277-286` `file_scan` flag comment overstates
that flag's reach: capture + `express.e:591-608`/`:28066-28115` show
`checkFileConfScan` gates **only** the logon `confScan` (which runs
`runSysCommand('N','S U')` per flagged conference) — menu `N` never
consults it. The multi-conference `SCAN`/`NSU` siblings and the logon
file-scan remain future slices over the deferred section-layer seam
(SYSTEM.md item 17).

---

## Notable findings

The most important behavioural gaps, in rough order of user impact:

1. **No-arg `J` is a one-line rejection in Rust, an interactive sub-flow in AE.** `J` with no/invalid/out-of-range argument drops into an interactive prompt in AmiExpress (`Conference Number (1-2): `). Rust replaces these with single-line usage/error messages (`Usage: J <conference-number>`, `Invalid conference number.`) and returns to the menu. (Bare `R` had the same shape but was brought to the legacy prompt-first readMSG loop in slice B10.)

2. **`J 99` (out of range): silent fall-through-to-Main vs interactive re-prompt.** Rust prints the legacy "no access" string and then *silently joins the first accessible conference*, whereas AE re-prompts and stays put. Rust reaches a legacy string under a condition the legacy never uses for it. The live AE transcript contradicts treating `J 99` as a no-access notice.

3. **Unknown-command handling is a NOTICE on both sides — not silent-vs-notice.** Correcting the earlier source-derived parity note (which described AE as silent): AE emits `No such command!!  Use '?' for command list.` (blank-line framed); Rust emits `Unknown command. Type G to log off.`. The difference is wording and framing (COSMETIC), not presence-vs-absence.

4. **~~`N` is a real new-files scan (AquaScan) in AE, but `Unknown` in Rust~~ Resolved (slice D9).** `N` now runs the NextScan new-files scan — the AquaScan door experience, byte-pinned to the dedicated two-pass capture `comparison/transcripts/ae_tierd_newfiles.txt` (see [N — new-files scan](#n--new-files-scan-aquascan-door-slice-d9)). The interim "Tier B removed the mail binding, scan deferred" state this finding described is over.

5. **The Rust menu advertises retired commands it then rejects.** `RP`/`FW`/`K`/`MV`/`EH` are still listed in the Rust main menu (MESSAGES and MAIL ADMIN sections) but every one now returns `Unknown command.`. The menu text was not updated when Tier B B8 retired them — an internal menu/dispatcher inconsistency with no equivalent in AE.

6. **`mins. left` shows `0` in Rust vs `599` in AE — seed data, not logic.** Both use identical arithmetic and rendering; the Rust sysop seed leaves `time_limit_per_call = 0`, so the prompt reads `0`. Cosmetic, but visually jarring.

7. **AE streams richer post-login presentation than Rust.** AmiExpress streams more than Rust during login: the paginated login mail scan (two `(Pause)...Space To Resume` gates), the per-conference stats block (`Total messages` / `Last message auto scanned` / `Last message read`), and the *fuller* user-stats / Uploads-Downloads-Ratio screen (Rust renders only the six-row subset). (The graphics question and the six-row login user-stats screen are already in place.)

8. **AE's menu is an elaborate per-conference ANSI art file; Rust's is a compact embedded ASCII block.** AE loads a configurable `/X`-style ANSI logo + bracketed coloured command grid ending in `Now attending to user: sysop`; Rust embeds a fixed `.oO(===[ NextExpress :: MAIN MENU ]===)Oo.` box + figlet + uncoloured list, with no "Now attending" trailer.

9. **`MS` pagination is the remaining gap.** The scan output (header, conference/message-base banners, `No mail today!`, listing table) and the read-it-now prompt + drop-into-read are an exact wire match; the residual gap is the mid-scan `checkForPause()` pagination. (The `Found Mail!` line belongs to AE's single-conf/auto-join path, which `MS` never reaches — a non-issue for MS parity.)

10. **`S` shares an exact six-row core but Rust implements only a subset.** The shared rows are byte-identical, yet Rust omits the `Area Name`/`Caller Num.` lead-in, the `Online Baud`/CPS/`Protocol`/`Sysop Here` block, and the entire Uploads/Downloads ratio table (deferred to slice A11).

11. **`VER` drops AE's `Registered to NONE.` line.** A deliberate omission (NextExpress has no registration-key concept); the rest of the banner is cosmetic re-framing plus a deliberate behavioural departure on the UTF-8-vs-Latin-1 `©` byte-encoding (resolved by the wire-encoding policy — see AGENTS.md).

12. **~~`G` (plain) always logs off in Rust~~ Resolved in part (slice D5/Ga).** Plain `G` now runs the genuine `checkFlagged()` confirm: with files flagged it prints `You have flagged files still not downloaded.` / `Do you leave without them? (y/N)?` (single-key `yesNo`, default N), returning to the menu on `N`; `G Y`, a `Y` answer, and the nothing-flagged case log off — matching AE, byte-pinned to `comparison/transcripts/ae_tierd_g_confirm.txt`. (Earlier note's claim that AE returns to the menu when *nothing* is flagged was a source misreading — an empty list returns `1` and logs off.) **Resolved further (slice D5-banner):** every `G` logoff now emits `saveFlagged()`'s `** AutoSaving File Flags **` banner + `<BEL>` before the goodbye, unconditionally (even with nothing flagged, live-confirmed in `ae_tierd_g_empty.txt`). **Resolved further (slice D5-persist):** cross-session flag persistence (per-slot, via the `FlaggedStore` port; durable under SQLite) and the logon `** Flagged File(s) Exist **` banner have landed. `saveHistory()` + the `dump` partial-downloads file remain deferred to the file-transfer slice.

13. **Several Rust-only notice lines have no legacy counterpart.** `Authenticated.` (post-login), `Message not found.` (failed `R <num>`), `Invalid conference number.` (`J abc`), and `Message aborted.` (blank subject) are all notices Rust emits where AE is silent or interactive. Individually minor; collectively they make Rust chattier than the legacy on edge paths.

---

## Cross-cutting wire-format notes

| Item | AmiExpress | NextExpress | Verdict |
|---|---|---|---|
| Line terminator | `\b\n` (Amiga E CR LF) | `\r\n` | ✓ Identical on the wire. |
| ANSI colour in prompts | Liberal (`[32m`/`[33m`/`[36m` in To/Subject/Private/menu) | Present in header/join/toggles/menu-prompt; **plain** on the `E`/`C` To/Subject/Private prompts | Gap — add ANSI to the compose prompts. |
| Trailing blank lines | Notices often end `\b\n\b\n`; one extra `\r\n` after toggles | Mixed — some notices have it, some don't | Audit & normalise. |
| Yes/No default | `yesNo(2)` → `(y/N)?` default **N** (the common case, e.g. Private) | Hard-coded `(y/N)?`, default **N** | ✓ Default matches; gap is ANSI + the rarer `yesNo(1)` default-Y sites still to decorate. |
| "Sysop only" denied | Varies per gate | Single `You do not have permission to perform that operation.` | Acceptable. |
| Source-not-found for K/MV/EH | n/a (these are R-sub-prompt verbs in AE, not menu commands) | `No such message in this base.` (now reachable only inside the `R` sub-prompt) | Greenfield; keep. |

## Recommended fixes & sequencing

Carried over from the original parity table. The R sub-prompt, the `MS`
multi-conf walk, the login graphics question, the login user-stats screen, the
`MS` read-it-now prompt + drop-into-read, and the ruler / `Msg. Options:` editor
for `E`/`C` — all once listed as pending — have since shipped. What remains,
roughly in order of effort vs. parity gain:

1. **Quick text fixes** (`wire_text.rs`, byte-for-byte parity, tiny diffs): E recipient-no-access → end with `!` + trailing blank (`express.e:10838`); EALL/ALL → distinct per-addressing notices (`:10806`); add the second trailing `\r\n` to `Goodbye`.
2. **ANSI on the compose prompts** — decorate the `E`/`C` `To:` / `Subject:` / `Private` prompts to match the legacy `(Enter)='ALL'?` / `(Blank)=abort?` boxes. Defaults already match; do **not** flip them.
3. **Interactive no-arg sub-flows** (largest interaction-model gap): bare `J` → `Conference Number (1-N): ` prompt (blank = abort). (Bare `R` → prompt-first readMSG loop: done in slice B10.)
4. **Menu hygiene** — stop advertising retired `RP`/`FW`/`K`/`MV`/`EH` in the menu asset (they now reject as Unknown), and reconsider the chatty Rust-only notices (item 13 above).
5. **`mins. left`** — seed a real per-call time budget so the prompt stops showing `0`.
6. **Login / `S` parity** (larger): flagged-file confirm on plain `G` (+ `saveFlagged`/`saveHistory`); the richer `S` screen (Area/Caller lead-in, baud/CPS/protocol/sysop block, Uploads/Downloads ratio table) beyond the six-row subset already shown at login; login mail-scan pagination.
7. **Tier D** — restore `N` as the new-files scan (currently Unknown).

## Methodology / sources of truth

- **Our side:** `rust/src/app/menu_command.rs` (parser), `rust/src/app/menu_flow/` (dispatch + sub-flows), `rust/src/app/wire_text.rs` (byte literals).
- **Legacy side:** `amiexpress/express.e` — `PROC internalCommand…` procedures and the helpers they call (`enterMSG`, `readMSG`, `commentToSYSOP`, `searchNewMail`, `joinConf`, `edit`).
- **Live evidence:** the segmented telnet transcripts in [`comparison/transcripts/`](./comparison/transcripts/), captured by the drivers in [`comparison/harness/`](./comparison/harness/). Each row reflects the exact byte sequence each side emits live; where the live wire contradicts a source reading, the live transcript wins (noted inline).