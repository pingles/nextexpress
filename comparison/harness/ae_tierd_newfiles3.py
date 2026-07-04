#!/usr/bin/env python3
"""Tier D capture — third `N` pass: date-prompt token family + `!x` overshoot.

ae_tierd_newfiles2.txt refuted PLAUSIBLE #2 for `T` (accepted at the date
prompt, scans today) and #8's `!1` wording (`the last file`, no count). This
pass pins the rest of those two families before the Rust fix:

  P2y  `Y` at the date prompt   (yesterday, if accepted like T)
  P2s  `S` at the date prompt   (since-last-call, if accepted)
  P2x  `!2` at the date prompt  (newest-pair, if accepted)
  P8j  `N !99` overshoot        (header count: requested 99 vs saturated?)

Same driving rules as ae_tierd_newfiles.py; clean `G Y` logoff.
"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ae_tierc import (  # noqa: E402
    LOG, to_menu, to_pattern, connect_until_node, HOST, PORT,
    MENU_SENTINEL, PAUSE_SENTINEL, read_until_any, emit,
)
from bbsdrive import render  # noqa: E402

MORE = b"uit:"
FLAG = b"to flag:"
DIRS = b"=None ?"
NSCONF = b"sure "
DATE = b" ?\x1b[0m "

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
    out_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/ae_tierd_newfiles3.txt"
    bbs, banner = connect_until_node(HOST, PORT, log=LOG)
    try:
        to_pattern(bbs, "graphics -> A", b"A\r", b"Name:", maxwait=60)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assword", maxwait=40)
        to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=120)
        to_menu(bbs, "ensure conf 2 (Amiga, seeded)", b"J 2\r")

        scenario(bbs, "P2y: bare N -> Y at date prompt -> dir 2",
                 b"N\r", date=[b"Y\r"], dirs=[b"2\r"])
        scenario(bbs, "P2s: bare N -> S at date prompt -> dir 2",
                 b"N\r", date=[b"S\r"], dirs=[b"2\r"])
        scenario(bbs, "P2x: bare N -> !2 at date prompt -> dir 2",
                 b"N\r", date=[b"!2\r"], dirs=[b"2\r"])
        scenario(bbs, "P8j: N !99 (overshoot header count)", b"N !99\r")
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
