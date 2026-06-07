# Login-sequence parity fixes

Parity fixes to the **logged-on session bring-up** sequence — the steps
between a successful password and the first menu prompt. These are not
menu commands; they belong with the foundation logon flow (the
auto-rejoin, the daily-budget init, the bulletins) rather than any
command family. The shipped pieces live in
[`session.allium`](../specs/session.allium) (the state machine, `EnterMenu`)
and are driven by `rust/src/app/session_driver.rs`.

## Slice L1 — logon conference scan (multi-conference new-mail scan)

**Status: Todo.** Documented + spec groundwork + live behaviour captured
2026-06-07; implementation deferred to its own session.

### Why this exists

The legacy runs a **multi-conference** mail scan at logon: `confScan()`
(`amiexpress/express.e:28066`), invoked from the `SUBSTATE_DISPLAY_BULL`
logon state (`:28564`), before the auto-rejoin
(`SUBSTATE_DISPLAY_CONF_BULL`, `:28574`). It surfaces "you have new mail
in conference X" before the menu opens.

NextExpress today scans only the **single** conference it auto-rejoins
(`session_driver.rs:162` `auto_rejoin_conference`, then `scan_mail_on_join`
for that one conference, `:181`). The multi-conference walk is missing,
and as a result the per-conference `mail_scan` flag the `CF` command
(Slice C5) edits is consulted by nothing — this slice is its consumer.

(There is **no** `CS` command — see Skipped slices in
[SLICES.md](../SLICES.md). This is the *logon-time* scan only.)

### What the legacy actually does (validated live — important)

The live capture (below) shows the logon scan is **the same machinery as
the `MS` command**, not a distinct UI:

- It walks every accessible conference and runs the same per-base
  `searchNewMail` the `MS` command renders: the
  `Scanning conferences for mail...` header, a `Scanning Conference:
  <name> - ` banner per conference, then either `No mail today!` or the
  `Type / From / Subject / Msg` listing table.
- When a base has matched mail it **drops into the same
  `Would you like to read it now (Y/n)?` offer** `MS` uses — confirmed
  live (an earlier assumption that it was summary-only was wrong).
- It differs from `MS` only in: (1) it scans **only** bases whose
  membership has `mail_scan` set (legacy `checkMailConfScan`, `:28095`),
  whereas `MS` forces every base; (2) it runs at logon, not on demand.
- It scans **by coordinate** and does not join or move the current
  conference (`joinConf(..., confScan=TRUE)` leaves `currentConf`
  untouched, `:4997`). The separate auto-rejoin then places the user in
  their home conference *after* the scan.

**Vehicle:** reuse the existing `scan_all_mail` use case + the `MS`
handler's rendering and read-it-now flow (`app/menu/scan_all_mail.rs`,
`app/menu_flow/scan_all_mail.rs`, `wire_text::render_scan_*`), adding a
`mail_scan` filter and driving it at logon. Do **not** use the
`conferences.allium:ConferenceScan` join-walk engine (Rust
`start/step_conference_scan`, foundation Slice 33): it *joins* each
conference (creating visits, running name-type promotion and bulletin
side effects) which the legacy coordinate scan does not. That engine has
no caller and doesn't match the legacy — a candidate for removal, but
removing it is **out of scope** for L1.

### Spec groundwork (already landed)

- `messaging.allium:ScanConferencesOnLogon` — mirrors `ScanAllMail` (the
  `MS` rule) but fires `MailScanRequested` only for bases whose
  membership `mail_scan` is set, gated to `onboarded` and non-quick-logon.
  Driver-raised via `ConferenceLogonScanRequested`, the same way `MS`
  raises `ScanAllMailRequested`.
- (`conferences.allium:ConferenceScan` rules left unchanged — not the
  vehicle, see above.)

### In scope

- Drive the logon scan from `session_driver.rs` **before** the auto-rejoin:
  raise `ConferenceLogonScanRequested`, run `scan_all_mail` filtered to
  `mail_scan`-enabled bases, render the `MS` output, and route matched
  mail through the existing read-it-now → `read_subprompt` flow.
- Keep the auto-rejoin as the separate following step; ensure the home
  conference is not scanned twice (logon scan by coordinate + auto-rejoin
  scan-on-join overlap — reconcile, e.g. skip the rejoin's scan since the
  logon scan already covered every flagged base).
- Quick-logon skips the scan (mirror `ShowConferenceBulletin`).

### Out of scope

- `MAILSCAN_PROMPT` "Scan for Mail (Y/n)" gate (`:28075`) — node tooltype.
- The post-scan part-upload check (`:28117-28147`) — Tier I.
- Removing the unused `ConferenceScan` join-walk engine — separate cleanup.

### Legacy references

- `confScan()` — `amiexpress/express.e:28066-28149`
- logon call site — `:28564` (`SUBSTATE_DISPLAY_BULL`)
- the flag gate — `checkMailConfScan`, `:572`; used at `:28095`
- per-base scan inside `joinConf` (no join when `confScan=TRUE`) — `:4997`, `:5119-5128`
- the auto-rejoin it precedes — `:28574`

### Validated legacy behaviour (FS-UAE reference, 2026-06-07)

Captured with `mail_scan` enabled on both conferences and one new message
seeded for the sysop in conference 2 ("Amiga"). Exact bytes of the
session-B logon scan (the leading `*****` is the password echo):

```
\r\n\r\nScanning conferences for mail...\r\n\r\n
\x1b[32mScanning Conference\x1b[33m: \x1b[0mNew Users - No mail today!\r\n
\x1b[32mScanning Conference\x1b[33m: \x1b[0mAmiga - \r\n\r\n
\x1b[32mType     From                           Subject                Msg    \r\n
\x1b[33m-------  -----------------------------  ---------------------  -------\r\n
\x1b[0mPublic   SYSOP                          Logon scan test        \x1b[0m000003\r\n\r\n
Would you like to read it now \x1b[32m(\x1b[33mY\x1b[32m/\x1b[33mn\x1b[32m)\x1b[32m?\x1b[0m
```

Notes: the header, `Scanning Conference: <name> - ` banner, `No mail
today!`, the listing table, the zero-padded `Msg` column, and the
read-it-now prompt are all **byte-identical** to the already-shipped `MS`
rendering — confirming the reuse path. (The empty-mailbox logon scan seen
during the C5 probe was the same header followed by a `(Pause)` and then
the auto-rejoin.)

### Tests (for the implementation session)

- Use-case: `scan_all_mail` filtered to `mail_scan`-enabled bases (flagged
  → scanned, unflagged-with-mail → skipped).
- Driver: logon runs the scan before the auto-rejoin; the home conference
  is scanned exactly once.
- Telnet smoke: log in with seeded new mail in a flagged second
  conference → the `Scanning conferences for mail...` output surfaces it
  (banner + listing + read-it-now) before the menu; an unflagged
  conference with mail is skipped.
- `cargo mutants` on the changed driver/use-case code.
