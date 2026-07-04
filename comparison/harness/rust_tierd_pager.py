#!/usr/bin/env python3
"""Pager-verb slice verification — replay the door's More?-verb probes
(ae_tierd_help_audit.txt PK/PL/PC/PCC, 2026-07-04) against a running
NextExpress binary. Labels mirror the reference capture so
diff_tierd_probes.py can pair the windows.

Not replayed: PN (the held-`n` prefix is probe-pinned in-tree and its
reference window carries timeout-recovery choreography), PO (`O` runs a
WHO display on the door — deferred until a who's-online surface
exists, a documented divergence), W1/W2 (the unported configurator).

Usage: python3 rust_tierd_pager.py [PORT] [OUT]
"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from rust_tierd_newfiles import (  # noqa: E402
    read_until_any, to_pattern, to_menu, scenario, emit, LOG,
)
from bbsdrive import BBS, render  # noqa: E402

HOST = "127.0.0.1"
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 2323
OUT = sys.argv[2] if len(sys.argv) > 2 else "/tmp/rust_tierd_pager.txt"


def main():
    bbs = BBS(HOST, PORT, idle=0.5, maxwait=10)
    try:
        clean, _ = read_until_any(bbs, [b"Graphics (Y/n)? "], maxwait=15)
        emit("CONNECT", None, clean, "BANNER")
        to_pattern(bbs, "graphics -> Y", b"Y\r", b"Name: ", maxwait=15)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assWord", maxwait=15)
        to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=30)

        scenario(bbs, "PK: F A, K at first More? (Skip dir)",
                 b"F A\r", more=[b"K", b"Q"])
        scenario(bbs, "PL: F 1, L at first More? (Reload dir)",
                 b"F 1\r", more=[b"L", b"Q"])
        scenario(bbs, "PC: F 1, C at first More? (Clear)",
                 b"F 1\r", more=[b"C", b"Q"])
        scenario(bbs, "PCC: F 1, Ctrl-C at first More? (quit any time)",
                 b"F 1\r", more=[b"\x03"])
    finally:
        try:
            bbs.send(b"G\r")
            for _ in range(6):
                clean = bbs.read_idle(idle=1.0, maxwait=8)
                LOG.append("\n@@@@@ LOGOFF round @@@@@")
                LOG.append(render(clean))
                if clean == b"":
                    break
        except OSError:
            pass
        bbs.close()
        with open(OUT, "w") as f:
            f.write("\n".join(LOG))
        print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
