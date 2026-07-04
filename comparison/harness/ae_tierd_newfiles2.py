#!/usr/bin/env python3
"""Tier D capture — AquaScan `N` follow-up probes for the D9 PLAUSIBLE list.

Slice D9 shipped thirteen provisional behaviours (design
2026-07-03-n-newfiles-scan-design.md §1.2; COMMAND_PARITY.md "N — new-files
scan" PLAUSIBLE table), each quarantined behind its own const/test. This pass
probes the live reference for the observable ones so each can be CONFIRMED or
REFUTED against what NextExpress ships:

  P2  (a) `T` typed AT the date prompt         (Rust: Error in date!)
  P3  (b) junk `FOO` at the Directories prompt (Rust: F's Error in input!)
  P1  (c) inline `N <date> H`                  (Rust: HOLD-substituted header)
  P10 (d) inline out-of-range dir `N <date> 9` (Rust: F's highest-dir error)
  P6  (e) year-less `N 06-15`                  (Rust: current year default)
  P11 (f) trailing junk after a valid date     (Rust: Error in date!)
  P7  (g) calendar-invalid `13-40-26` at the prompt (Rust: Error in date!)
  P4  (h) inline letter span `N <date> A`      (Rust: shared span resolver)
  P8  (i) `N !1` wording                       (Rust: `the last 1 files`)

Same driving rules as ae_tierd_newfiles.py: conference 2 (Amiga, seeded),
LINE-read date prompt matched by its own sentinel, More? answered explicitly,
and a clean `G Y` logoff (FS-UAE node-spin hazard).
"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ae_tierc import (  # noqa: E402
    LOG, to_menu, to_pattern, connect_until_node, HOST, PORT,
    MENU_SENTINEL, PAUSE_SENTINEL, read_until_any, emit,
)
from bbsdrive import render  # noqa: E402

MORE = b"uit:"            # More? (Y/n/ns), ... (Q)uit:
FLAG = b"to flag:"        # File name(s) to flag:
DIRS = b"=None ?"         # Directories: ... (Enter)=None ?
NSCONF = b"sure "         # Non-stop scrolling! Are you sure (Y/n)?
DATE = b" ?\x1b[0m "      # Date: ... (Enter)=MM-DD-YY ?<ESC>[0m<SP>

DEFAULTS = {MORE: b"Y", FLAG: b"\r", DIRS: b"\r", NSCONF: b"n", DATE: b"\r"}


def scenario(bbs, label, send, more=(), flag=(), dirs=(), nsconf=(), date=(),
             maxrounds=80, maxwait=75):
    """Send `send`, then answer every known sub-prompt from per-kind queues
    (safe defaults when a queue runs dry) until the menu prompt returns.
    Recovers to the menu (Q, CR) on surprise so scenarios cannot cascade."""
    queues = {MORE: list(more), FLAG: list(flag), DIRS: list(dirs),
              NSCONF: list(nsconf), DATE: list(date)}
    bbs.send(send)
    collected = b""
    status = "RAN OUT OF ROUNDS"
    for _ in range(maxrounds):
        clean, hit = read_until_any(
            bbs, [MENU_SENTINEL, FLAG, DIRS, NSCONF, MORE, DATE,
                  PAUSE_SENTINEL],
            maxwait=maxwait)
        collected += clean
        if hit == MENU_SENTINEL:
            status = "MENU"
            break
        if hit == PAUSE_SENTINEL:
            collected += b"<<<harness: SPACE>>>"
            bbs.send(b" ")
            continue
        if hit is None:
            status = "TIMEOUT (no sentinel)"
            break
        q = queues.get(hit)
        ans = q.pop(0) if q else DEFAULTS[hit]
        collected += b"<<<harness answers %s: %s>>>" % (hit, ans)
        bbs.send(ans)
    if status != "MENU":
        for esc in (b"Q", b"\r", b"q\r"):
            bbs.send(esc)
            clean, hit = read_until_any(bbs, [MENU_SENTINEL], maxwait=25)
            collected += b"<<<recovery %s>>>" % esc + clean
            if hit == MENU_SENTINEL:
                status += " +RECOVERED"
                break
    emit(label, send, collected, status)
    return collected


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/ae_tierd_newfiles2.txt"
    bbs, banner = connect_until_node(HOST, PORT, log=LOG)
    try:
        to_pattern(bbs, "graphics -> A", b"A\r", b"Name:", maxwait=60)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assword", maxwait=40)
        to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=120)
        to_menu(bbs, "ensure conf 2 (Amiga, seeded)", b"J 2\r")

        # (a) P2: T at the date prompt — accepted (today-scan) or error?
        scenario(bbs, "P2a: bare N -> T at date prompt -> dir 2",
                 b"N\r", date=[b"T\r"], dirs=[b"2\r"])
        # (b) P3: junk at the Directories prompt.
        scenario(bbs, "P3b: bare N -> Enter date -> FOO at dirs prompt",
                 b"N\r", date=[b"\r"], dirs=[b"FOO\r"])
        # (c) P1: inline HOLD-dir scan.
        scenario(bbs, "P1c: N 12-30-26 H (inline HOLD scan)",
                 b"N 12-30-26 H\r")
        # (d) P10: inline out-of-range dir.
        scenario(bbs, "P10d: N 01-01-26 9 (inline out-of-range dir)",
                 b"N 01-01-26 9\r")
        # (e) P6: year-less date, inline.
        scenario(bbs, "P6e: N 06-15 (year omitted)", b"N 06-15\r")
        # (f) P11: trailing junk after a valid date at the prompt.
        scenario(bbs, "P11f: bare N -> '01-01-26 X' at date prompt -> dir 2",
                 b"N\r", date=[b"01-01-26 X\r"], dirs=[b"2\r"])
        # (g) P7: calendar-invalid but date-shaped input at the prompt.
        scenario(bbs, "P7g: bare N -> 13-40-26 at date prompt -> dir 2",
                 b"N\r", date=[b"13-40-26\r"], dirs=[b"2\r"])
        # (h) P4: inline letter span A.
        scenario(bbs, "P4h: N 01-01-26 A (inline all-dirs span)",
                 b"N 01-01-26 A\r")
        # (i) P8: !1 header wording (pluralisation).
        scenario(bbs, "P8i: N !1 (newest-1 header wording)", b"N !1\r")
    finally:
        try:
            bbs.send(b"G Y\r")
            for _ in range(8):
                clean = bbs.read_idle(idle=1.5, maxwait=12)
                LOG.append("\n@@@@@ LOGOFF round @@@@@")
                LOG.append(render(clean))
                if clean == b"":
                    break
        except OSError:
            pass
        bbs.close()
        with open(out_path, "w") as f:
            f.write("\n".join(LOG))
        print(f"wrote {out_path}")


if __name__ == "__main__":
    main()
