#!/usr/bin/env python3
"""Tier D capture — file listings (F / FR) against seeded DIR files.

Board prep (done before this runs): Conf02 patched to NDIRS=2; Conf02/Dir1
seeded with 28 entries in the authentic upload-writer row format
(express.e:19454 — name \\l\\s[13], status char at col 13, size \\r\\d[7] at
col 14, date MM-DD-YY at col 23, description at col 33, continuation lines
indented 33 spaces); Conf02/Dir2 seeded with 3 fresh entries; Conf01/Dir1
left empty. Blobs for four entries dropped in Conf02/Uploads/.

Scenarios: F 1 (paginated stream), F 2 (upload-dir T: copy branch), F A,
F U, F H (hold scan as sysop), F 99 (No such directory.), F 1 NS (non-stop),
bare F -> Directories prompt answered with 1, bare F -> Enter (=none abort),
FR 1 / FR A (reverse, Tier D3 fodder), then J 1 -> F 1 (empty Dir1) and
bare F in a single-dir conference, J 2 to restore the usual rejoin target.

Every session ends with a clean `G Y` logoff (FS-UAE node-spin hazard).
"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ae_tierc import (  # noqa: E402
    LOG, to_menu, to_pattern, connect_until_node, HOST, PORT,
)
from bbsdrive import render  # noqa: E402

DIR_PROMPT = b"=none? "


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/ae_tierd_files.txt"
    bbs, banner = connect_until_node(HOST, PORT, log=LOG)
    try:
        to_pattern(bbs, "graphics -> A", b"A\r", b"Name:", maxwait=60)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assword", maxwait=40)
        to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=120)

        to_menu(bbs, "ensure conf 2 (Amiga, seeded)", b"J 2\r")

        to_menu(bbs, "F1: F 1 (28-entry dir, expect pagination)", b"F 1\r",
                maxwait=60)
        to_menu(bbs, "F2: F 2 (upload dir, T: copy branch)", b"F 2\r")
        to_menu(bbs, "F3: F A (all dirs walk)", b"F A\r", maxwait=60)
        to_menu(bbs, "F4: F U (upload dir shortcut)", b"F U\r")
        to_menu(bbs, "F5: F H (hold scan, sysop access)", b"F H\r")
        to_menu(bbs, "F6: F 99 (out of range)", b"F 99\r")
        to_menu(bbs, "F7: F 1 NS (non-stop, no pause expected)", b"F 1 NS\r",
                maxwait=60)

        to_pattern(bbs, "F8: bare F -> Directories prompt", b"F\r",
                   DIR_PROMPT, maxwait=30)
        to_menu(bbs, "F8b: answer 1 at the prompt", b"1\r", maxwait=60)
        to_pattern(bbs, "F9: bare F again -> prompt", b"F\r",
                   DIR_PROMPT, maxwait=30)
        to_menu(bbs, "F9b: Enter alone (=none, abort)", b"\r")

        to_menu(bbs, "R1: FR 1 (reverse listing)", b"FR 1\r", maxwait=60)
        to_menu(bbs, "R2: FR A (reverse all)", b"FR A\r", maxwait=60)

        to_menu(bbs, "E1: J 1 (New Users conf, empty Dir1)", b"J 1\r")
        to_menu(bbs, "E2: F 1 (empty DIR file)", b"F 1\r")
        to_pattern(bbs, "E3: bare F in single-dir conf -> prompt", b"F\r",
                   DIR_PROMPT, maxwait=30)
        to_menu(bbs, "E3b: Enter alone (abort)", b"\r")

        to_menu(bbs, "restore rejoin target: J 2", b"J 2\r")
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
