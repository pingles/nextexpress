# Stage 5 Tester-A session logs — D8 FS — target: NextExpress

Driver: character-at-a-time (one byte sent, idle-snapshot read after each keystroke;
technique ported from `comparison/harness/ae_tierd_probes.py` P3 + IAC handling from
`bbsdrive.py`). Server: NextExpress `@127.0.0.1:27228` (build `3e36056`, in-memory,
seeded sysop/sysop). Login: `Y\r` to `ANSI Graphics (Y/n)?` (line-read — needs Enter),
`sysop\r` to name, `sysop\r` to password, read to menu sentinel `mins. left): `.

**UTF-8 gate:** every byte received across all seven scenarios (incl. the login banner's
`\xc2\xa9` = `©` U+00A9) decoded as valid UTF-8. No raw Latin-1, no BLOCKER-class byte.

**Menu command line is a line-read:** every printable keystroke at the menu prompt echoes
the exact byte typed (no-echo would be a hotkey/lone-key read; not the case here). The
terminating `\r` (Enter) is echoed as `\r\n` (CRLF) immediately followed by the command's
response, then the full MAIN MENU redisplay ending in the `mins. left): ` sentinel.

`[MENU]` in the tables below = the static MAIN MENU redisplay block, byte-identical across
scenarios; its full repr is given once in the Appendix. The conference tag in the sentinel
is `[1:Main]` for conf 1 and `[2:Programming]` for conf 2.

---

## Session log — S1 — target: NextExpress
a) Scenario / inputs: S1 bare FS (grammar G1); inputs = [ `F`, `S`, `<Enter>` ]
b) Target: NextExpress @127.0.0.1:27228, conference 1 (Main)
c) Per-step (one row per byte, char-at-a-time):

| # | input (key/line) | observed bytes after keystroke | echo? | terminator | notes |
|---|---|---|---|---|---|
| 1 | `F` | `"F"` | yes | — | line-read echoes the byte |
| 2 | `S` | `"S"` | yes | — | line-read echoes the byte |
| 3 | `\r` (Enter) | `"\r\n\r\nCommand requires higher access.\r\n"` + `[MENU]` ending `…mins. left): ` | yes | CRLF | CR echoed as CRLF; FS recognised as command head, denied by ACS gate; returns to Main menu |

Session end: connection closed [Tester-A]

---

## Session log — S2 — target: NextExpress
a) Scenario / inputs: S2 lowercase fs (grammar G2); inputs = [ `f`, `s`, `<Enter>` ]
b) Target: NextExpress @127.0.0.1:27228, conference 1 (Main)
c) Per-step:

| # | input (key/line) | observed bytes after keystroke | echo? | terminator | notes |
|---|---|---|---|---|---|
| 1 | `f` | `"f"` | yes | — | line-read echoes the byte |
| 2 | `s` | `"s"` | yes | — | line-read echoes the byte |
| 3 | `\r` (Enter) | `"\r\n\r\nCommand requires higher access.\r\n"` + `[MENU]` ending `…mins. left): ` | yes | CRLF | lowercase `fs` matched identically to uppercase head; same ACS deny + Main menu |

Session end: connection closed [Tester-A]

---

## Session log — S3 — target: NextExpress
a) Scenario / inputs: S3 FS with numeric arg (grammar G3); inputs = [ `F`, `S`, `<Space>`, `1`, `<Enter>` ]
b) Target: NextExpress @127.0.0.1:27228, conference 1 (Main)
c) Per-step:

| # | input (key/line) | observed bytes after keystroke | echo? | terminator | notes |
|---|---|---|---|---|---|
| 1 | `F` | `"F"` | yes | — | line-read echoes |
| 2 | `S` | `"S"` | yes | — | line-read echoes |
| 3 | `<Space>` | `" "` | yes | — | separator echoed |
| 4 | `1` | `"1"` | yes | — | numeric arg echoed |
| 5 | `\r` (Enter) | `"\r\n\r\nCommand requires higher access.\r\n"` + `[MENU]` ending `…mins. left): ` | yes | CRLF | head taken with trailing `1`; same ACS deny + Main menu |

Session end: connection closed [Tester-A]

---

## Session log — S4 — target: NextExpress
a) Scenario / inputs: S4 FS with junk arg (grammar G4); inputs = [ `F`, `S`, `<Space>`, `x`, `y`, `z`, `<Enter>` ]
b) Target: NextExpress @127.0.0.1:27228, conference 1 (Main)
c) Per-step:

| # | input (key/line) | observed bytes after keystroke | echo? | terminator | notes |
|---|---|---|---|---|---|
| 1 | `F` | `"F"` | yes | — | line-read echoes |
| 2 | `S` | `"S"` | yes | — | line-read echoes |
| 3 | `<Space>` | `" "` | yes | — | separator echoed |
| 4 | `x` | `"x"` | yes | — | junk arg echoed |
| 5 | `y` | `"y"` | yes | — | junk arg echoed |
| 6 | `z` | `"z"` | yes | — | junk arg echoed |
| 7 | `\r` (Enter) | `"\r\n\r\nCommand requires higher access.\r\n"` + `[MENU]` ending `…mins. left): ` | yes | CRLF | head taken with junk `xyz`; same ACS deny + Main menu (junk arg ignored, no error) |

Session end: connection closed [Tester-A]

---

## Session log — S5 — target: NextExpress
a) Scenario / inputs: S5 bare FS from a different conference (grammar G5); inputs = [ `J`, `<Enter>`, `2`, `<Enter>`, `F`, `S`, `<Enter>` ]
b) Target: NextExpress @127.0.0.1:27228, starting conf 1 (Main), joining conf 2 (Programming)
c) Per-step:

| # | input (key/line) | observed bytes after keystroke | echo? | terminator | notes |
|---|---|---|---|---|---|
| 1 | `J` | `"J"` | yes | — | line-read echoes |
| 2 | `\r` (Enter) | `"\r\nConference Number (1-2): "` | yes | CRLF | J with no inline arg → sub-prompt for conf number (CR echoed as CRLF) |
| 3 | `2` | `"2"` | yes | — | conf number echoed at the sub-prompt (also a line-read) |
| 4 | `\r` (Enter) | `"\r\n\r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m Programming\r\n\r\nNo new mail.\r\n"` + `[MENU]` ending `…[2:Programming] Menu (0 mins. left): ` | yes | CRLF | joined conf 2; menu redisplays with `[2:Programming]` tag |
| 5 | `F` | `"F"` | yes | — | line-read echoes |
| 6 | `S` | `"S"` | yes | — | line-read echoes |
| 7 | `\r` (Enter) | `"\r\n\r\nCommand requires higher access.\r\n"` + `[MENU]` ending `…[2:Programming] Menu (0 mins. left): ` | yes | CRLF | FS from conf 2 → identical ACS deny; behaviour independent of current conference |

Session end: connection closed [Tester-A]

---

## Session log — S6 — target: NextExpress
a) Scenario / inputs: S6 FS wrapped in leading/trailing spaces (grammar G6 — expected-divergence row, no expectation asserted); inputs = [ `<Space>`, `<Space>`, `F`, `S`, `<Space>`, `<Space>`, `<Enter>` ]
b) Target: NextExpress @127.0.0.1:27228, conference 2 (Programming — current from S5)
c) Per-step:

| # | input (key/line) | observed bytes after keystroke | echo? | terminator | notes |
|---|---|---|---|---|---|
| 1 | `<Space>` | `" "` | yes | — | leading space echoed |
| 2 | `<Space>` | `" "` | yes | — | leading space echoed |
| 3 | `F` | `"F"` | yes | — | line-read echoes |
| 4 | `S` | `"S"` | yes | — | line-read echoes |
| 5 | `<Space>` | `" "` | yes | — | trailing space echoed |
| 6 | `<Space>` | `" "` | yes | — | trailing space echoed |
| 7 | `\r` (Enter) | `"\r\n\r\nCommand requires higher access.\r\n"` + `[MENU]` ending `…mins. left): ` | yes | CRLF | NextExpress trims surrounding whitespace and still recognises `FS` as the command head → same ACS deny |

Session end: connection closed [Tester-A]
(Recorded, no judgement — cross-marker holds the reference log.)

---

## Session log — S7 — target: NextExpress
a) Scenario / inputs: S7 FSX with no separator (grammar G8 — expected-divergence row, no expectation asserted); inputs = [ `F`, `S`, `X`, `<Enter>` ]
b) Target: NextExpress @127.0.0.1:27228, conference 2 (Programming)
c) Per-step:

| # | input (key/line) | observed bytes after keystroke | echo? | terminator | notes |
|---|---|---|---|---|---|
| 1 | `F` | `"F"` | yes | — | line-read echoes |
| 2 | `S` | `"S"` | yes | — | line-read echoes |
| 3 | `X` | `"X"` | yes | — | line-read echoes |
| 4 | `\r` (Enter) | `"\r\nUnknown command. Type G to log off.\r\n"` + `[MENU]` ending `…mins. left): ` | yes | CRLF | `FSX` NOT matched as `FS` head → distinct "Unknown command" path (single leading CRLF, not the double-CRLF deny); returns to menu |

Session end: connection closed [Tester-A]
(Recorded, no judgement — cross-marker holds the reference log.)

---

## Appendix — full `[MENU]` redisplay repr (byte-identical across S1–S7)

The `\r` (Enter) response in every scenario appends this static MAIN MENU block after the
command's own line. Full Python-repr (conf tag shown as `[1:Main]`; conf 2 substitutes
`2` / `Programming`):

```
"  .oO(==============[ NextExpress :: MAIN MENU ]==============)Oo.\r\n       __  __      _        __  __\r\n      |  \\/  |__ _(_)_ _   |  \\/  |___ _ _ _  _\r\n      | |\\/| / _` | | ' \\  | |\\/| / -_) ' \\ || |\r\n      |_|  |_\\__,_|_|_||_| |_|  |_\\___|_||_\\_,_|\r\n  `----------------------------------------------------------------'\r\n\r\n  MESSAGES\r\n    R <n>    Read message number <n>\r\n    MS       Scan all visible messages\r\n    E [to]   Enter a message\r\n    C        Comment to sysop\r\n\r\n  CONFERENCES\r\n    J <n>    Join conference number <n>\r\n    JM <n>   Join message base number <n>\r\n    <        Join previous accessible conference\r\n    >        Join next accessible conference\r\n    <<       Join previous message base\r\n    >>       Join next message base\r\n    CF       Edit conference scan flags\r\n\r\n  FILES\r\n    F [n]    List files (<n>=dir, A=all, U=upload, ?=help)\r\n    N [date] Scan for new files since <date>\r\n    Z <str>  Zippy search file descriptions for <str>\r\n    A        List your flagged files\r\n\r\n  SESSION\r\n    S        User stats\r\n    T        Show current time\r\n    Q        Toggle quiet mode\r\n    M        Toggle ANSI colour\r\n    X        Toggle expert mode\r\n    ?        Re-display this menu\r\n    ^ <topic> Help on a topic\r\n    H        Help\r\n    VER      Version information\r\n    G        Goodbye (log off)\r\n\x1b[0m\x1b[35mNextExpress \x1b[0m[\x1b[36m1\x1b[34m:\x1b[36mMain\x1b[0m] Menu (\x1b[33m0\x1b[0m mins. left): "
```

Session end: connection closed cleanly [Tester-A]
```
