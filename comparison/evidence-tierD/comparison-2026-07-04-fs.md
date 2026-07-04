# Comparison — D8 / `FS` (file status, FAITHFUL DENY) — 2026-07-04

Server: NextExpress @127.0.0.1:27228 (commit 3e36056, slice/d8-fs) · in-memory sysop/sysop
Reference: FS-UAE @127.0.0.1:27227 (container nextexpress-ref-d8fs, node 0; budget 3/5 opens)
Scenarios: 7 (grammar rows G1–G6, G8 + happy path folded into G1; Q1 quarantined/undriven)
Interactive human glance: **not applicable** — FS is a pure deny, no sub-prompt/pager/hotkey (§10.7)

## Verdict

**CLEAN** — no un-triaged divergence. Both double-blind cross-markers returned PASS. Every
flagged divergence is either a **documented labelled departure** (G6/G8, `COMMAND_PARITY.md`
§DEPARTURE :1012–1017) or **known pre-existing drift** (pre-prompt blank line, conf name/number,
©→UTF-8 re-encoding). The load-bearing surface — the deny line — is byte-identical.

## Load-bearing result

The deny wire `\r\n\r\nCommand requires higher access.\r\n` is **byte-identical on both
targets** for S1–S5 (G1–G5): identical two-CRLF lead-in, identical text (single-spaced,
trailing period, no trailing space), identical CRLF terminator. Confirmed on both sides:
case-insensitive (`fs`=`FS`), trailing arg ignored (`FS 1`, `FS xyz`), **conference-
independent** (deny holds after a conference join). Menu command line is a line-read with
per-key echo + CRLF Enter on both. UTF-8 gate green on both (only high-bit byte `0xA9`→
U+00A9 © in the out-of-scope login banner).

## Divergences (from cross-marks, most-severe first — all INFO)

| id | scenario | severity | kind | NextExpress | FS-UAE | disposition |
|---|---|---|---|---|---|---|
| 1 | S6 `  FS  ` | INFO | labelled-departure **G6** | trims leading/trailing WS → `FileStatus` → deny | leading WS → unrecognised → `No such command!!  Use '?' for command list.` | Documented `COMMAND_PARITY.md:1016`; pre-existing project-wide tokenizer-fold (the `  A `→`AlterFlags` class), not FS-introduced |
| 2 | S7 `FSX` | INFO | labelled-departure **G8** | `Unknown command. Type G to log off.` (1 leading CRLF) | `No such command!!  Use '?' for command list.` (2 leading/3 trailing CRLF) | Documented `COMMAND_PARITY.md:1017`; both reject `FSX`≠`FS` head — pre-existing unknown-command wording departure |
| 3 | S1–S5 deny trailer | INFO | known-drift | `…access.\r\n` | `…access.\r\n\r\n` (extra pre-prompt blank line) | Global cosmetic divergence `COMMAND_PARITY.md:380` (`express.e:28589`), shared by every command |
| 4 | S5 conference | INFO | volatile/config (§10.6) | conf 2 "Programming"; testers joined conf 2 | conf 2 "Amiga"; tester joined conf 1 | Fixture conf name/number differ; both established a non-default conf; FS deny conference-independent on both. Sub-prompt `\r\nConference Number (1-2): ` byte-identical |
| 5 | login banner (out of scope) | INFO | encoding | `\xc2\xa9` = valid UTF-8 U+00A9 | raw `0xA9` Latin-1 | Intended AGENTS.md re-encoding; same code point; **NOT mojibake** — NextExpress wire is correct |
| 6 | login graphics prompt (out of scope) | INFO | login precondition | `ANSI Graphics (Y/n)?` line-read | `(A/r/n)?` needs CR | Outside FS scope; both logged in cleanly |

Under-recording note (not a divergence): Tester-A ellipsized the S5 join banner and did not
log its own auto-rejoin default. Low stakes — the load-bearing FS-deny-after-join *is*
evidenced on both sides; noted for a future Tester-A tighten.

## Completeness-critic findings

All applicable grammar rows exercised on **both** testers: G1, G2, G3, G4, G5, G6, G8 —
none uncovered on either side. Edge-probe battery: out-of-range/non-numeric → G3/G4; unknown
token → G8; trailing junk → G4; whitespace-surrounded → G6; mojibake/terminator → UTF-8 gate
PASS. Non-applicable by construction (pure deny): sub-prompt accept/reject, empty-collection
gate, pager, lone-key-vs-line-read. **Q1** (granted opt=0 accounting table) correctly NOT
driven — unreachable behind the ACS gate, owned by A11/Tier-I. **Verdict: PASS**, no re-loop.

## Board hygiene (§10.8)

Tester-B: one serialized node hold, 3/<5 telnet opens (2 spent diagnosing the graphics-prompt
CR requirement), conference 2 restored before logoff, clean `G Y` logoff confirmed
(`** AutoSaving File Flags **` `\x07` `Click...` → carrier EOF). Node released cleanly.

## Evidence

- Tester-A (NextExpress) session logs: `comparison/evidence-tierD/stage5-testerA-fs-sessionlogs.md`
- Tester-B (FS-UAE) session logs: `comparison/evidence-tierD/fs-testerB-session-2026-07-04.md`
- Capture transcript: `comparison/transcripts/ae_tierd_fs.txt`
- Design: `designs/2026-07-04-fs-design.md`
