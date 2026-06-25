#!/usr/bin/env python3
"""Probe — plain `G` logoff with an EMPTY flag set.

The alterflags cleanup left the persisted flag set empty, so this login
should show NO `** Flagged File(s) Exist **` banner, and a plain `G`
(nothing flagged) should reach saveFlagged unconditionally
(checkFlagged returns 1 for an empty list, express.e:12674 `ENDPROC 1`)
— printing `** AutoSaving File Flags **` + BEL before logoff. Grounds the
D5-banner correction (banner on every G logoff, not only when flagged).
"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ae_tierc import LOG, to_menu, to_pattern, emit, HOST, PORT  # noqa: E402
from ae_session import connect_until_node  # noqa: E402
from bbsdrive import render  # noqa: E402


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/ae_tierd_g_empty.txt"
    bbs, banner = connect_until_node(HOST, PORT, log=LOG)
    try:
        to_pattern(bbs, "graphics -> A", b"A\r", b"Name:", maxwait=60)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assword", maxwait=40)
        login = to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=120)
        emit("BANNER CHECK (expect ABSENT — flags cleared)", None,
             b"** Flagged File(s) Exist ** STILL PRESENT"
             if b"Flagged File(s) Exist" in login
             else b"(no saved-flag banner at login - as expected)", "info")

        # plain G with nothing flagged: no confirm, straight to logoff.
        bbs.send(b"G\r")
        logoff = bbs.read_idle(idle=2.0, maxwait=20)
        emit("plain G (empty flag set) -> logoff", b"G\r", logoff,
             "banner=%s" % ("YES" if b"AutoSaving" in logoff else "NO"))
    finally:
        try:
            bbs.send(b"G Y\r")
            for _ in range(4):
                clean = bbs.read_idle(idle=1.5, maxwait=8)
                LOG.append("\n@@@@@ SAFETY LOGOFF round @@@@@")
                LOG.append(render(clean))
                if clean == b"":
                    break
        except OSError:
            pass
        bbs.close()
        with open(out_path, "w") as f:
            f.write("\n".join(LOG))
        print("wrote", out_path)


if __name__ == "__main__":
    main()
