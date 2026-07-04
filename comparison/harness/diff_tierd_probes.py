#!/usr/bin/env python3
"""Slice D9 follow-up — pair the Rust probe capture
(rust_tierd_newfiles2.txt) against the reference PLAUSIBLE-item probes
(ae_tierd_newfiles2.txt + ae_tierd_newfiles3.txt) probe-for-probe, using
the same windowing/normalisation rules as diff_tierd_newfiles.py.

Usage: python3 diff_tierd_probes.py [RUST_TXT] [AE2_TXT] [AE3_TXT]
Exit status: number of probes with a REAL divergence (0 = parity holds).
"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from diff_tierd_newfiles import (  # noqa: E402
    parse, window, norm_rust, norm_ae, content_mode, show_diff,
    AE_MENU_PROMPT, RUST_MENU_ART, RUST_MENU_PROMPT,
)

RUST = sys.argv[1] if len(sys.argv) > 1 else "comparison/transcripts/rust_tierd_newfiles2.txt"
AE2 = sys.argv[2] if len(sys.argv) > 2 else "comparison/transcripts/ae_tierd_newfiles2.txt"
AE3 = sys.argv[3] if len(sys.argv) > 3 else "comparison/transcripts/ae_tierd_newfiles3.txt"


def main():
    rust = parse(RUST, "utf-8")
    ae = parse(AE2, "latin-1") + parse(AE3, "latin-1")
    by_key = {}
    for blk in ae:
        if blk["text"] is not None:
            by_key.setdefault(blk["key"], blk)

    skip = {"CONNECT", "graphics -> Y", "graphics -> A", "name -> sysop",
            "password -> POST-LOGIN", "ensure conf 2 (Amiga, seeded)",
            "LOGOFF round"}
    real = 0
    for rb in rust:
        if rb["key"] in skip or rb["text"] is None:
            continue
        ab = by_key.get(rb["key"])
        if ab is None:
            print(f"{'NO REFERENCE':<18} {rb['label']}")
            continue
        rw = window(rb["text"], [RUST_MENU_ART.decode(), RUST_MENU_PROMPT.decode()])
        aw = window(ab["text"], [AE_MENU_PROMPT.decode("latin-1")])
        rn, an = norm_rust(rw), norm_ae(aw)
        if rn == an:
            print(f"{'MATCH':<18} {rb['label']}")
            continue
        rc, ra = content_mode(rn)
        ac, aa = content_mode(an)
        if rc == ac:
            print(f"{'MATCH (content)':<18} {rb['label']}  [pager positions differ: "
                  f"rust {ra}, ref {aa}]")
            continue
        real += 1
        print(f"{'DIVERGENCE':<18} {rb['label']}")
        print(show_diff(rc, ac, "rust(normalised)", "reference(normalised)"))
        print()

    print(f"\n{real} probe(s) with unexplained divergence")
    sys.exit(min(real, 125))


if __name__ == "__main__":
    main()
