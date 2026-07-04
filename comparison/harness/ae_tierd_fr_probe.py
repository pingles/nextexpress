#!/usr/bin/env python3
"""Tier D capture — does the AquaScan door accept the spaced `F R` form?

The door's own `F ?` help advertises `F R - Same as above but use reverse
scanning` and `F [R] dir [Q] [NS] - Start scanning immediately`, but slice
D3 kept `F R` on the Argument-error path citing the *internal* dispatcher
(`express.e:28310` — separate `F`/`FR` tokens), which is the shadowed stock
path, not the door. This pass asks the door itself:

  FR1  `F R`     (spaced reverse, prompt form)
  FR2  `F R 2`   (spaced reverse + dir, immediate form)
  FR3  `F 1 Q`   (the Q token the same UNVERIFIED block records)

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
    out_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/ae_tierd_fr_probe.txt"
    bbs, banner = connect_until_node(HOST, PORT, log=LOG)
    try:
        to_pattern(bbs, "graphics -> A", b"A\r", b"Name:", maxwait=60)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assword", maxwait=40)
        to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=120)
        to_menu(bbs, "ensure conf 2 (Amiga, seeded)", b"J 2\r")

        scenario(bbs, "FR1: F R (spaced reverse, prompt form)",
                 b"F R\r", dirs=[b"2\r"])
        scenario(bbs, "FR2: F R 2 (spaced reverse + dir)", b"F R 2\r")
        scenario(bbs, "FR3: F 1 Q (the Q token)", b"F 1 Q\r",
                 more=[b"Q"])
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
