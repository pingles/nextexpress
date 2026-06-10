#!/usr/bin/env python3
"""Tier D capture 4 — pin the six UNVERIFIED AquaScan pager corners.

Targets (all door-side, board as-shipped): mid-list lone `n` at More?;
`?` help at More?; ns-confirm answered `n`; junk menu args (`F XYZ`);
`A`/`U`/`H` typed at the bare-F Directories prompt. (The seventh —
`F A` with an empty first dir — runs as a separate session via
ae_tierd_aquascan5.py after Dir1 is temporarily truncated.)

Sub-prompt map and hazards as in ae_tierd_aquascan3.py.
Every session ends with a clean `G Y` logoff (FS-UAE node-spin hazard).
"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ae_tierc import (  # noqa: E402
    LOG, to_menu, to_pattern, connect_until_node, HOST, PORT,
)
from bbsdrive import render  # noqa: E402
from ae_tierd_aquascan3 import scenario  # noqa: E402


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/ae_tierd_aquascan4.txt"
    bbs, banner = connect_until_node(HOST, PORT, log=LOG)
    try:
        to_pattern(bbs, "graphics -> A", b"A\r", b"Name:", maxwait=60)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assword", maxwait=40)
        to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=120)
        to_menu(bbs, "ensure conf 2 (Amiga, seeded)", b"J 2\r")

        scenario(bbs, "U1: F 1 -> mid-list lone n at first More?",
                 b"F 1\r", more=[b"n"])
        scenario(bbs, "U2: F 1 -> ? at More? (help), then Q",
                 b"F 1\r", more=[b"?", b"Q"])
        scenario(bbs, "U3: F 1 -> ns -> confirm n (decline), then Q",
                 b"F 1\r", more=[b"ns", b"Q"], nsconf=[b"n"])
        scenario(bbs, "U4: F XYZ (junk menu arg)", b"F XYZ\r")
        scenario(bbs, "U5: bare F -> A at Directories prompt",
                 b"F\r", dirs=[b"A\r"], more=[b"Q", b"Q"])
        scenario(bbs, "U6: bare F -> U at Directories prompt",
                 b"F\r", dirs=[b"U\r"], more=[b"Q"])
        scenario(bbs, "U7: bare F -> H at Directories prompt",
                 b"F\r", dirs=[b"H\r"])
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
