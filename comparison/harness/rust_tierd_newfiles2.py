#!/usr/bin/env python3
"""Slice D9 follow-up — replay the PLAUSIBLE-item probe batteries
(ae_tierd_newfiles2.txt + ae_tierd_newfiles3.txt, 2026-07-04) against a
running NextExpress binary. Labels mirror the reference captures
probe-for-probe so diff_tierd_probes.py can pair the windows.

Server: cargo run --manifest-path rust/Cargo.toml -- nextexpress.toml
        (127.0.0.1:2323, seeded sysop/sysop, demo corpus in conference 1).

Usage: python3 rust_tierd_newfiles2.py [PORT] [OUT]
"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from rust_tierd_newfiles import (  # noqa: E402
    read_until_any, to_pattern, to_menu, scenario, emit, LOG,
)
from bbsdrive import BBS  # noqa: E402

HOST = "127.0.0.1"
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 2323
OUT = sys.argv[2] if len(sys.argv) > 2 else "/tmp/rust_tierd_newfiles2.txt"


def main():
    bbs = BBS(HOST, PORT, idle=0.5, maxwait=10)
    try:
        clean, _ = read_until_any(bbs, [b"Graphics (Y/n)? "], maxwait=15)
        emit("CONNECT", None, clean, "BANNER")
        to_pattern(bbs, "graphics -> Y", b"Y\r", b"Name: ", maxwait=15)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assWord", maxwait=15)
        to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=30)

        # ---- ae_tierd_newfiles2.txt battery ----
        scenario(bbs, "P2a: bare N -> T at date prompt -> dir 2",
                 b"N\r", date=[b"T\r"], dirs=[b"2\r"])
        scenario(bbs, "P3b: bare N -> Enter date -> FOO at dirs prompt",
                 b"N\r", date=[b"\r"], dirs=[b"FOO\r"])
        scenario(bbs, "P1c: N 12-30-26 H (inline HOLD scan)",
                 b"N 12-30-26 H\r")
        scenario(bbs, "P10d: N 01-01-26 9 (inline out-of-range dir)",
                 b"N 01-01-26 9\r")
        scenario(bbs, "P6e: N 06-15 (year omitted)", b"N 06-15\r")
        scenario(bbs, "P11f: bare N -> '01-01-26 X' at date prompt -> dir 2",
                 b"N\r", date=[b"01-01-26 X\r"], dirs=[b"2\r"])
        scenario(bbs, "P7g: bare N -> 13-40-26 at date prompt -> dir 2",
                 b"N\r", date=[b"13-40-26\r"], dirs=[b"2\r"])
        scenario(bbs, "P4h: N 01-01-26 A (inline all-dirs span)",
                 b"N 01-01-26 A\r")
        scenario(bbs, "P8i: N !1 (newest-1 header wording)", b"N !1\r")

        # ---- ae_tierd_newfiles3.txt battery ----
        scenario(bbs, "P2y: bare N -> Y at date prompt -> dir 2",
                 b"N\r", date=[b"Y\r"], dirs=[b"2\r"])
        scenario(bbs, "P2s: bare N -> S at date prompt -> dir 2",
                 b"N\r", date=[b"S\r"], dirs=[b"2\r"])
        scenario(bbs, "P2x: bare N -> !2 at date prompt -> dir 2",
                 b"N\r", date=[b"!2\r"], dirs=[b"2\r"])
        scenario(bbs, "P8j: N !99 (overshoot header count)", b"N !99\r")
    finally:
        try:
            bbs.send(b"G\r")
            for _ in range(6):
                clean = bbs.read_idle(idle=1.0, maxwait=8)
                LOG.append("\n@@@@@ LOGOFF round @@@@@")
                from bbsdrive import render
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
