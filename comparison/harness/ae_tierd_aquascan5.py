#!/usr/bin/env python3
"""Tier D capture 5 — `F A` with an empty first dir (AquaScan transition).

Run AFTER truncating Conf02/Dir1 to zero bytes (docker cp an empty file
over it); restore from comparison/evidence-tierD/fixtures/Dir1 afterwards.
Pins what AquaScan emits between a `Nothing found!` dir and the next
dir's scan during an `A` span, plus `F 1` alone on the emptied dir for
the footer/More? question.

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
    out_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/ae_tierd_aquascan5.txt"
    bbs, banner = connect_until_node(HOST, PORT, log=LOG)
    try:
        to_pattern(bbs, "graphics -> A", b"A\r", b"Name:", maxwait=60)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assword", maxwait=40)
        to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=120)
        to_menu(bbs, "ensure conf 2 (Dir1 emptied)", b"J 2\r")

        scenario(bbs, "V1: F A with empty dir 1 -> populated dir 2",
                 b"F A\r", more=[b"Y", b"Q"])
        scenario(bbs, "V2: F 1 (emptied dir alone)", b"F 1\r")
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
