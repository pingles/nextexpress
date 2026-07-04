#!/usr/bin/env python3
"""F-grammar fix verification — replay the spaced-`R` / `Q` probes
(ae_tierd_fr_probe.txt FR1-FR3, 2026-07-04) against a running
NextExpress binary. Labels mirror the reference capture so
diff_tierd_probes.py can pair the windows.

The W1/W2 configurator probes are deliberately NOT replayed: `F W` /
`N W` stay on the Argument-error path (config is TOML — the documented
permanent departure; the door opens its `AquaScan Configuration` UI).

Usage: python3 rust_tierd_frgrammar.py [PORT] [OUT]
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
OUT = sys.argv[2] if len(sys.argv) > 2 else "/tmp/rust_tierd_frgrammar.txt"


def main():
    bbs = BBS(HOST, PORT, idle=0.5, maxwait=10)
    try:
        clean, _ = read_until_any(bbs, [b"Graphics (Y/n)? "], maxwait=15)
        emit("CONNECT", None, clean, "BANNER")
        to_pattern(bbs, "graphics -> Y", b"Y\r", b"Name: ", maxwait=15)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assWord", maxwait=15)
        to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=30)

        scenario(bbs, "FR1: F R (spaced reverse, prompt form)",
                 b"F R\r", dirs=[b"2\r"])
        scenario(bbs, "FR2: F R 2 (spaced reverse + dir)", b"F R 2\r")
        scenario(bbs, "FR3: F 1 Q (the Q token)", b"F 1 Q\r",
                 more=[b"Q"])
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
