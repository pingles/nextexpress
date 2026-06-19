#!/usr/bin/env python3
"""Tier D supplementary capture — `Z` getDirSpan answers not in zippy run 1.

Run 1 (`ae_tierd_zippy.txt`) captured the search prompt, the internal
`getDirSpan('')` prompt, a single-dir-number answer, `A` (all dirs), the
blank `=none` abort, and a no-match dir. This run pins the remaining
`getDirSpan` answers so D4 can render the full internal prompt path
faithfully:

  * `U` (upload = highest dir) — `getDirSpan` :26881-26884.
  * `H` (hold dir) — `internalCommandZ` :26196-26199 (`Scanning directory
    HOLD`); seeded hold is empty.
  * an out-of-range number — `getDirSpan` :26904-26906 (`No such
    directory.`), which differs from AquaScan's `The highest directory
    number is N!`.

Searches target Conf02 dir 2 (`DEMO` matches MYDEMO.DMS there) so the `U`
(=highest dir, 2) answer shows a real match. Every session ends with a
clean `G Y` logoff (FS-UAE node-spin hazard).
"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ae_tierc import LOG, to_menu, to_pattern, connect_until_node  # noqa: E402
from bbsdrive import render  # noqa: E402
from ae_tierd_zippy import zippy  # noqa: E402


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/ae_tierd_zippy2.txt"
    bbs, banner = connect_until_node(HOST_PORT[0], HOST_PORT[1], log=LOG)
    try:
        to_pattern(bbs, "graphics -> A", b"A\r", b"Name:", maxwait=60)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assword", maxwait=40)
        to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=120)
        to_menu(bbs, "ensure conf 2 (Tier D fixture)", b"J 2\r")

        # ZU: U = upload dir = highest dir (2). DEMO matches MYDEMO.DMS
        # in dir 2 — pins the U answer + its "Scanning directory 2".
        zippy(bbs, "ZU: Z DEMO (inline) -> U (upload/highest dir)",
              b"Z DEMO\r", dirspan=b"U\r")

        # ZH: H = hold dir (empty) — pins "Scanning directory HOLD".
        zippy(bbs, "ZH: Z DEMO (inline) -> H (hold dir)",
              b"Z DEMO\r", dirspan=b"H\r")

        # ZOOR: out-of-range number (max is 2) — pins "No such directory."
        zippy(bbs, "ZOOR: Z DEMO (inline) -> 5 (out of range)",
              b"Z DEMO\r", dirspan=b"5\r")

        # ZZERO: zero is also out of range (Val('0')=0 < 1).
        zippy(bbs, "ZZERO: Z DEMO (inline) -> 0 (out of range)",
              b"Z DEMO\r", dirspan=b"0\r")
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


HOST_PORT = ("127.0.0.1", 6023)

if __name__ == "__main__":
    main()
