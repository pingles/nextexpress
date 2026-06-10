#!/usr/bin/env python3
"""Tier C capture 3 — the one remaining gap: a clean `<` SUCCESS at a menu
prompt (accessible lower neighbour exists). Also `J -1` (negative arg).

Ends with a clean `G Y` logoff and drains until the server drops the line.
"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ae_tierc import (  # noqa: E402
    LOG, to_menu, to_pattern, connect_until_node, HOST, PORT,
)
from bbsdrive import render  # noqa: E402


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/ae_tierc3.txt"
    bbs, banner = connect_until_node(HOST, PORT, log=LOG)
    try:
        to_pattern(bbs, "graphics -> A", b"A\r", b"Name:", maxwait=40)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assword", maxwait=40)
        to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=90)

        to_menu(bbs, "reposition: J 2 (to conf 2)", b"J 2\r")
        to_menu(bbs, "GAP: < from conf 2 (clean success -> conf 1)", b"<\r")
        to_menu(bbs, "BACK: J 2 (to conf 2 again)", b"J 2\r")
        to_pattern(bbs, "NEG: J -1 -> prompt?", b"J -1\r", b"Number (1-2): ", maxwait=30)
        to_menu(bbs, "NEG: blank abort", b"\r")
        # leave rejoin pointer at conf 2 for any future session
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
