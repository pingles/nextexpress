#!/usr/bin/env python3
"""Tier D slice D8 capture — `FS` (file status), the genuine internal command.

`FS` reaches internalCommandFS() -> fileStatus(0) (express.e:24872 -> :24141).
NOT door-shadowed: Z/A/FS have no AquaScan icons in BBS:Commands/BBSCmd/
(slices/cmds-files-list.md:43), so the token dispatches straight to the
internal command. No door-vs-source (§10.3) decision.

fileStatus(0) prints a per-conference Uploads/Downloads accounting table and
returns to the menu with NO sub-prompts (express.e:24153-24188):
  blank, `[32m              Uploads                 Downloads`,
  blank, the column header (`Bytes` variant unless CREDITBYKB toggle),
  the `----` rule, one row per accessible conference, trailing blank.

Because there are no sub-prompts the "sub-prompt eats the next line" hazard is
low; each probe is a single command driven straight to the menu prompt.

Edge battery (thin — FS takes no params):
  Row 13  case:          `fs` vs `FS`  (same result?)
  Row 7/14 trailing/arg: `FS 1`, `FS xyz`  (params ignored / error / other?)
  Row 10  single-conf:   covered by the row-set count of the bare `FS`.
Deny path (RESULT_NOT_ALLOWED, express.e:24873) is UNTESTABLE with sysop
sec 255 -> resolved from source in the evidence note, tagged
extrapolated-from-source.

Every session ends with a clean `G Y` logoff (FS-UAE node-spin hazard).
"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ae_tierc  # noqa: E402
from ae_tierc import (  # noqa: E402
    LOG, to_menu, to_pattern, emit, MENU_SENTINEL,
)
from ae_session import connect_until_node  # noqa: E402
from bbsdrive import render  # noqa: E402

# D8 reference container `nextexpress-ref-d8fs` maps host 27227 -> 6023.
HOST, PORT = "127.0.0.1", 27227
ae_tierc.HOST, ae_tierc.PORT = HOST, PORT


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/ae_tierd_fs.txt"
    bbs, banner = connect_until_node(HOST, PORT, log=LOG)
    try:
        # ---- login ----
        to_pattern(bbs, "graphics -> A", b"A\r", b"Name:", maxwait=60)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assword", maxwait=40)
        to_menu(bbs, "password -> POST-LOGIN (to first menu prompt)",
                b"sysop\r", maxwait=120)

        # Normalise the auto-rejoin target; capture FS from a known conf.
        to_menu(bbs, "ensure conf 2 (Amiga, seeded) before FS", b"J 2\r")

        # ---- D8 money capture: bare FS, no sub-prompts ----
        to_menu(bbs, "D8: FS (bare, from conf 2) -> accounting table", b"FS\r")

        # ---- edge battery ----
        # Row 13: lowercase token — same result as FS?
        to_menu(bbs, "D8 edge R13: fs (lowercase)", b"fs\r")
        # Row 7/14: trailing numeric arg — internalCommandFS takes no params.
        to_menu(bbs, "D8 edge R7: FS 1 (trailing numeric arg)", b"FS 1\r")
        # Row 7/14: junk inline arg.
        to_menu(bbs, "D8 edge R14: FS xyz (junk inline arg)", b"FS xyz\r")

        # ---- Row 10 cross-check: FS from a different current conf ----
        # Confirms the current-conf colour (n=3) tracks currentConf and the
        # row set is per-accessible-conference, not per-current-conf.
        to_menu(bbs, "join conf 1 (New Users) for FS colour cross-check",
                b"J 1\r")
        to_menu(bbs, "D8 xref: FS (from conf 1) -> current-conf colour shifts",
                b"FS\r")
        # Restore rejoin target so the next session isn't polluted.
        to_menu(bbs, "restore rejoin target: J 2", b"J 2\r")
    finally:
        # clean logoff — MANDATORY for the FS-UAE node
        try:
            bbs.send(b"G Y\r")
            for _ in range(8):
                clean = bbs.read_idle(idle=1.5, maxwait=12)
                LOG.append("\n@@@@@ LOGOFF round @@@@@")
                LOG.append(render(clean))
                if clean == b"":
                    break
                low = clean.lower()
                if b"no carrier" in low or b"goodbye" in low:
                    bbs.read_idle(idle=1.0, maxwait=4)
                    break
                if b"y/n" in low or b"sure" in low:
                    bbs.send(b"Y\r")
        except OSError:
            pass
        bbs.close()
        with open(out_path, "w") as f:
            f.write("\n".join(LOG))
        print(f"wrote {out_path}")


if __name__ == "__main__":
    main()
