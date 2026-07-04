# Stage 5 Tester-B session logs — slice D8 `FS` — LIVE FS-UAE reference

Target: FS-UAE @127.0.0.1:27227 (container `nextexpress-ref-d8fs`), node 0.
Date: 2026-07-04. Driver: `scratchpad/fs_driver.py` (adapted from `comparison/harness/ae_session.py` + `bbsdrive.py`).
Login: graphics `A\r`, Name `sysop\r`, Password `sysop\r` (masked `*****`), auto-rejoin conf 2 "Amiga", sec 255, menu sentinel `mins. left): `.
Connection budget: 3 telnet opens used of <5 (2 spent diagnosing a login-sequencing bug — graphics `(A/r/n)?` requires CR; bare `A` concatenates with the next line). One serialized session, all 7 scenarios + conf-restore + `G Y` logoff in a single node hold.

Encoding note: every observed byte in scenarios S1–S7 is 7-bit ASCII + ANSI CSI ESC sequences — all valid UTF-8, no byte ≥0x80. The only application byte ≥0x80 in the whole session is in the login banner: `0xA9` (Latin-1 `©`) in `Copyright ©2018-2023` — Latin-1 byte `0xA9` → UTF-8 code point U+00A9 `©`. (Telnet `\xff\xfd\x1f` IAC DO negotiation and the `\x07` BEL at logoff are protocol/control, not high-bit text.)

Line-read behaviour: the AmiExpress menu is a **line read** — every typed key echoes back identically as typed (`F`→`F`, space→space, digit→digit), and the Enter (`\r` sent) is echoed/terminated as **CRLF** (`\r\n`), followed by a blank line then the command output. Menu prompt redraw after every command: `\x1b[0m\x1b[35mNextExpress Reference \x1b[0m[\x1b[36m<conf#>\x1b[34m:\x1b[36m<confname>\x1b[0m] Menu (\x1b[33m597\x1b[0m mins. left): `.

FS is recognised as an internal command that is **ACS-denied even for sysop sec 255** — a pure deny (`Command requires higher access.`) with no sub-prompt/pager/hotkey surface. Trailing args (numeric or junk) are ignored — the deny fires on the command head regardless.

---

## Session log — S1 — target: FS-UAE
a) Scenario / inputs: S1 bare FS from current conference (conf 2 Amiga); inputs = [ `F`, `S`, `<Enter>` ]
b) Target: FS-UAE @127.0.0.1:27227
c) Per-step:
   | # | input (key/line) | observed bytes after keystroke | echo? | terminator | notes |
   |---|---|---|---|---|---|
   | 1 | `F` | `b'F'` | yes | — | menu line-read echoes key as typed |
   | 2 | `S` | `b'S'` | yes | — | echoes as typed |
   | 3 | `<Enter>` (`\r`) | `b'\r\n\r\nCommand requires higher access.\r\n\r\n' + <menu prompt [2:Amiga]>` | yes (CRLF) | CRLF (`\r\n`) | FS recognised, ACS-denied; returns to conf-2 menu |
Session end: (session continues) — clean `G Y` logoff at end of S7/restore.

## Session log — S2 — target: FS-UAE
a) Scenario / inputs: S2 lowercase fs; inputs = [ `f`, `s`, `<Enter>` ]
b) Target: FS-UAE @127.0.0.1:27227
c) Per-step:
   | # | input (key/line) | observed bytes after keystroke | echo? | terminator | notes |
   |---|---|---|---|---|---|
   | 1 | `f` | `b'f'` | yes | — | echoes lowercase as typed |
   | 2 | `s` | `b's'` | yes | — | echoes as typed |
   | 3 | `<Enter>` (`\r`) | `b'\r\n\r\nCommand requires higher access.\r\n\r\n' + <menu prompt [2:Amiga]>` | yes (CRLF) | CRLF | case-insensitive: same deny as uppercase FS |

## Session log — S3 — target: FS-UAE
a) Scenario / inputs: S3 FS with numeric trailing arg; inputs = [ `F`, `S`, `<Space>`, `1`, `<Enter>` ]
b) Target: FS-UAE @127.0.0.1:27227
c) Per-step:
   | # | input (key/line) | observed bytes after keystroke | echo? | terminator | notes |
   |---|---|---|---|---|---|
   | 1 | `F` | `b'F'` | yes | — | |
   | 2 | `S` | `b'S'` | yes | — | |
   | 3 | `<Space>` | `b' '` | yes | — | space echoes |
   | 4 | `1` | `b'1'` | yes | — | |
   | 5 | `<Enter>` (`\r`) | `b'\r\n\r\nCommand requires higher access.\r\n\r\n' + <menu prompt [2:Amiga]>` | yes (CRLF) | CRLF | trailing numeric arg ignored; same deny |

## Session log — S4 — target: FS-UAE
a) Scenario / inputs: S4 FS with junk trailing arg; inputs = [ `F`, `S`, `<Space>`, `x`, `y`, `z`, `<Enter>` ]
b) Target: FS-UAE @127.0.0.1:27227
c) Per-step:
   | # | input (key/line) | observed bytes after keystroke | echo? | terminator | notes |
   |---|---|---|---|---|---|
   | 1 | `F` | `b'F'` | yes | — | |
   | 2 | `S` | `b'S'` | yes | — | |
   | 3 | `<Space>` | `b' '` | yes | — | |
   | 4 | `x` | `b'x'` | yes | — | |
   | 5 | `y` | `b'y'` | yes | — | |
   | 6 | `z` | `b'z'` | yes | — | |
   | 7 | `<Enter>` (`\r`) | `b'\r\n\r\nCommand requires higher access.\r\n\r\n' + <menu prompt [2:Amiga]>` | yes (CRLF) | CRLF | trailing junk arg ignored; same deny |

## Session log — S5 — target: FS-UAE
a) Scenario / inputs: S5 bare FS from a DIFFERENT conference; inputs = [ `J`, `<Enter>`, `<conf#>`, `<Enter>`, `F`, `S`, `<Enter>` ]
   Deviation recorded: login auto-rejoins conf **2** (Amiga), so to satisfy "non-default conference" I joined conf **1** (New Users) rather than the literal `2` in the scenario script (joining 2 would not change conference). Load-bearing part preserved: FS run while a non-default conference is current.
b) Target: FS-UAE @127.0.0.1:27227
c) Per-step:
   | # | input (key/line) | observed bytes after keystroke | echo? | terminator | notes |
   |---|---|---|---|---|---|
   | 1 | `J` | `b'J'` | yes | — | J echoes as typed |
   | 2 | `<Enter>` (`\r`) | `b'\r\nConference Number (1-2): '` | yes (CRLF) | CRLF | `J` alone opens the conf-number **sub-prompt** (range shown 1-2) |
   | 3 | `1` | `b'1'` | yes | — | sub-prompt echoes the digit |
   | 4 | `<Enter>` (`\r`) | `b'\r\n\r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m New Users\r\n\r\n...Total messages...0...\r\n' + <menu prompt [1:New Users]>` | yes (CRLF) | CRLF | joined conf 1 "New Users" |
   | 5 | `F` | `b'F'` | yes | — | |
   | 6 | `S` | `b'S'` | yes | — | |
   | 7 | `<Enter>` (`\r`) | `b'\r\n\r\nCommand requires higher access.\r\n\r\n' + <menu prompt [1:New Users]>` | yes (CRLF) | CRLF | same deny in conf 1 — conference-independent |

## Session log — S6 — target: FS-UAE  [EXPECTED DIVERGENCE — recorded, not a defect]
a) Scenario / inputs: S6 FS wrapped in leading/trailing spaces; inputs = [ `<Space>`, `<Space>`, `F`, `S`, `<Space>`, `<Space>`, `<Enter>` ]
   (Run while conf 1 "New Users" current, immediately after S5.)
b) Target: FS-UAE @127.0.0.1:27227
c) Per-step:
   | # | input (key/line) | observed bytes after keystroke | echo? | terminator | notes |
   |---|---|---|---|---|---|
   | 1 | `<Space>` | `b' '` | yes | — | leading space echoes |
   | 2 | `<Space>` | `b' '` | yes | — | |
   | 3 | `F` | `b'F'` | yes | — | |
   | 4 | `S` | `b'S'` | yes | — | |
   | 5 | `<Space>` | `b' '` | yes | — | trailing space echoes |
   | 6 | `<Space>` | `b' '` | yes | — | |
   | 7 | `<Enter>` (`\r`) | `b"\r\n\r\nNo such command!!  Use '?' for command list.\r\n\r\n\r\n" + <menu prompt [1:New Users]>` | yes (CRLF) | CRLF | leading whitespace makes token unrecognised → **unknown-command** path, NOT the FS deny |

## Session log — S7 — target: FS-UAE  [EXPECTED DIVERGENCE — recorded, not a defect]
a) Scenario / inputs: S7 FSX with no separator; inputs = [ `F`, `S`, `X`, `<Enter>` ]
   (Run while conf 1 "New Users" current.)
b) Target: FS-UAE @127.0.0.1:27227
c) Per-step:
   | # | input (key/line) | observed bytes after keystroke | echo? | terminator | notes |
   |---|---|---|---|---|---|
   | 1 | `F` | `b'F'` | yes | — | |
   | 2 | `S` | `b'S'` | yes | — | |
   | 3 | `X` | `b'X'` | yes | — | |
   | 4 | `<Enter>` (`\r`) | `b"\r\n\r\nNo such command!!  Use '?' for command list.\r\n\r\n\r\n" + <menu prompt [1:New Users]>` | yes (CRLF) | CRLF | `FSX` no-separator token unrecognised → **unknown-command** path |

---

## Conference restore + logoff (capture-pollution guard + node hazard)
- Restore: `J` → sub-prompt `\r\nConference Number (1-2): ` → `2` → `b'\r\n\r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m Amiga\r\n...Total messages...3...' + <menu prompt [2:Amiga]>`. Auto-rejoin target restored to conf **2 (Amiga)** before logoff.
- Logoff: sent `G Y\r`. Observed `b'G Y\r\n\r\n** AutoSaving File Flags **\r\n\x07\r\nClick...\x1b[0m'` then socket EOF (carrier dropped). `** AutoSaving File Flags **` is the unconditional G-logoff banner; `Click...` is the hangup. Node released cleanly.

Session end: clean `G Y` logoff (conf 2 restored).
