#!/usr/bin/env python3
"""Tier C capture 4 — pin E's Val() prefix semantics live.

Scenarios: `J 2abc` (does Val parse the leading digits -> joins conf 2?),
`J +2` (is a leading '+' accepted?), and `2abc` typed at the interactive
conference prompt.
"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ae_tierc import (  # noqa: E402
    LOG, to_menu, to_pattern, connect_until_node, HOST, PORT,
)
from bbsdrive import render  # noqa: E402


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/ae_tierc4.txt"
    bbs, banner = connect_until_node(HOST, PORT, log=LOG)
    try:
        to_pattern(bbs, "graphics -> A", b"A\r", b"Name:", maxwait=40)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assword", maxwait=40)
        to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=90)

        to_menu(bbs, "reset: J 1", b"J 1\r")
        to_menu(bbs, "VAL1: J 2abc (leading digits?)", b"J 2abc\r")
        to_menu(bbs, "reset: J 1 again", b"J 1\r")
        to_menu(bbs, "VAL2: J +2 (leading plus?)", b"J +2\r")
        # if VAL2 opened a prompt, the next blank aborts; harmless if at menu
        to_menu(bbs, "VAL2b: blank (abort if prompted)", b"\r")
        to_pattern(bbs, "VAL3: J -> prompt", b"J\r", b"Number (1-2): ", maxwait=30)
        to_menu(bbs, "VAL3b: 2abc at the prompt", b"2abc\r")
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
