#!/usr/bin/env python3
"""Slice D9 follow-up — capture `F ?` from a running NextExpress binary and
diff it against the AquaScan reference (`ae_tierd_aquascan3.txt` S1,
:100-129), normalising the documented branding departures via the shared
`diff_tierd_newfiles` rules.

Regression check for the help-diagram indent fix (2026-07-04): the Rust
consts' trailing-`\\` continuations silently stripped the captured 5-space
diagram indent from `F ?` (7-space from `N ?`).

Usage: python3 rust_tierd_fhelp.py [PORT]   (server on 127.0.0.1:PORT)
Exit status: 0 = MATCH, 1 = divergence (diff printed).
"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from bbsdrive import BBS, strip_iac  # noqa: E402
from diff_tierd_newfiles import (  # noqa: E402
    parse, window, norm_rust, norm_ae, show_diff, AE_MENU_PROMPT,
    RUST_MENU_ART, RUST_MENU_PROMPT,
)
from rust_tierd_newfiles import read_until_any  # noqa: E402

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 2323
AE = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                  "..", "transcripts", "ae_tierd_aquascan3.txt")


def main():
    bbs = BBS("127.0.0.1", PORT, idle=0.5, maxwait=10)
    try:
        read_until_any(bbs, [b"Graphics (Y/n)? "], maxwait=15)
        bbs.send(b"Y\r")
        read_until_any(bbs, [b"Name: "], maxwait=15)
        bbs.send(b"sysop\r")
        read_until_any(bbs, [b"assWord"], maxwait=15)
        bbs.send(b"sysop\r")
        read_until_any(bbs, [b"mins. left): "], maxwait=30)
        bbs.send(b"F ?\r")
        clean, hit = read_until_any(bbs, [b"mins. left): "], maxwait=20)
        bbs.send(b"G\r")
    finally:
        bbs.close()

    rust_text = window(clean.decode("utf-8"),
                       [RUST_MENU_ART.decode(), RUST_MENU_PROMPT.decode()])

    s1 = next(b for b in parse(AE, "latin-1") if b["key"] == "S1")
    ae_text = window(s1["text"], [AE_MENU_PROMPT.decode("latin-1")])
    # The capture echoes the typed command; the in-scenario window starts
    # identically on both sides (`F ?\r\n` + form feed).
    rn, an = norm_rust(rust_text), norm_ae(ae_text)
    if rn == an:
        print("MATCH: F ? window is byte-identical modulo documented branding")
        return 0
    print("DIVERGENCE in the F ? window:")
    print(show_diff(rn, an, "rust(normalised)", "reference(normalised)"))
    return 1


if __name__ == "__main__":
    sys.exit(main())
