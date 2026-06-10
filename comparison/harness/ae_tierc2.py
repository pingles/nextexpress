#!/usr/bin/env python3
"""Tier C follow-up capture — close gaps left by ae_tierc.py state pollution.

Clean captures needed:
  1. `<` success path (from conf 2 -> conf 1) at a clean menu prompt
  2. `J 1.1` dotted arg at a clean menu prompt
  3. `J 99` / `J 0` / `J abc` DIRECT args -> do they open the interactive prompt?
  4. blank input at the prompt opened by `>` (edge) — silent abort?
"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ae_tierc import (  # noqa: E402
    LOG, to_menu, to_pattern, emit, connect_until_node, HOST, PORT,
    MENU_SENTINEL,
)


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/ae_tierc2.txt"
    bbs, banner = connect_until_node(HOST, PORT, log=LOG)
    try:
        to_pattern(bbs, "graphics -> A", b"A\r", b"Name:", maxwait=40)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assword", maxwait=40)
        to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=90)

        # auto-rejoin lands in conf 2 (Amiga); capture a clean `<` success
        to_menu(bbs, "GAP1: < from conf 2 (clean success path)", b"<\r")

        # now in conf 1 — clean dotted J
        to_menu(bbs, "GAP2: J 1.1 at clean menu (in conf 1)", b"J 1.1\r")
        to_menu(bbs, "GAP2b: J 2.1 then back to conf 1 via J 1", b"J 2.1\r")
        to_menu(bbs, "GAP2c: J 1 (reposition to conf 1)", b"J 1\r")

        # direct out-of-range / invalid args -> interactive prompt?
        to_pattern(bbs, "GAP3: J 99 direct -> prompt?", b"J 99\r", b"Number (1-2): ", maxwait=30)
        to_menu(bbs, "GAP3: blank abort", b"\r")
        to_pattern(bbs, "GAP3b: J 0 direct -> prompt?", b"J 0\r", b"Number (1-2): ", maxwait=30)
        to_menu(bbs, "GAP3b: blank abort", b"\r")
        to_pattern(bbs, "GAP3c: J abc direct -> prompt?", b"J abc\r", b"Number (1-2): ", maxwait=30)
        to_menu(bbs, "GAP3c: answer 2 (join Amiga from prompt)", b"2\r")

        # at conf 2 now: `>` edge -> prompt; blank input -> silent abort?
        to_pattern(bbs, "GAP4: > at conf 2 -> prompt?", b">\r", b"Number (1-2): ", maxwait=30)
        to_menu(bbs, "GAP4: blank abort at edge prompt", b"\r")

        # confirm we are still in conf 2 (prompt label) via a harmless T
        to_menu(bbs, "GAP4b: T (confirm still conf 2)", b"T\r")

        # `<` edge: go to conf 1 first, then `<` -> prompt, blank abort
        to_menu(bbs, "GAP5: J 1 (to conf 1)", b"J 1\r")
        to_pattern(bbs, "GAP5: < at conf 1 -> prompt?", b"<\r", b"Number (1-2): ", maxwait=30)
        to_menu(bbs, "GAP5: blank abort at < edge prompt", b"\r")

        # extra-token form: J 1 2 — what does legacy do?
        to_menu(bbs, "EXTRA: J 1 2 (two tokens)", b"J 1 2\r")
    finally:
        try:
            bbs.send(b"G Y\r")
            for _ in range(6):
                clean = bbs.read_idle(idle=1.5, maxwait=12)
                LOG.append("\n@@@@@ LOGOFF round @@@@@")
                from bbsdrive import render
                LOG.append(render(clean))
                if clean == b"":
                    break
                if b"y/n" in clean.lower():
                    bbs.send(b"Y\r")
        except OSError:
            pass
        bbs.close()
        with open(out_path, "w") as f:
            f.write("\n".join(LOG))
        print(f"wrote {out_path}")


if __name__ == "__main__":
    main()
