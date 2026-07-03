#!/usr/bin/env python3
"""Tier D capture — AquaScan `N` (new-files scan), the D9 ground truth.

The stock deployment ships AquaScan v1.0 door icons shadowing N (and CS, F,
FR, NSU, SCAN, SENT) in BBS:Commands/BBSCmd/ — so `N` runs the AquaScan door,
not internalCommandF. Prior passes captured only the N banner + date prompt
(ae_tierd_aquascan.txt A10, ae_tierd_aquascan3.txt S12, both timed out at the
date prompt); this pass answers everything and captures the actual scans.

N surface (established by the first run of this script, 2026-07-03):
  banner -> `--[ AquaScan v1.0 ... ]---------------[ 'n ?' for options ]--`
            (right label switches to `Copyright \xa9 1994 Aquarius` on the
            help screen and on `Argument error!`)
  date   -> `Date: (MM-DD-YY), (-X) Days, (R)everse, (Enter)=MM-DD-YY ? `
            LINE read; the default date is the DAY OF LAST CALL, not today;
            junk -> `Error in date!` -> menu
  dirs   -> `Directories: (1-2), (A)ll, (U)pload, (H)old, (Enter)=None ? `
            LINE read, same as bare F; Enter=None ABORTS silently to menu
  inline -> `N [S]|mm-dd[-yy]|T|Y|-x|!x|R [dir] [Q] [NS]` skips both prompts
            (dir defaults to U = upload dir); bad combos (e.g. `N R -1`) ->
            `Argument error! Type 'n ?' for help.`

Scan output reuses the F/SCAN frame shapes: `Scanning dir N for MM-DD-YY...
Ok!/Nothing found!`, date-group separators, `[ File #n ]` frames,
`[ End of File List ]`, and the F-style More? (no Skip-Conf verb: N is
current-conference only, unlike the SCAN/NSU siblings).

Every session ends with a clean `G Y` logoff (FS-UAE node-spin hazard).
"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ae_tierc import (  # noqa: E402
    LOG, to_menu, to_pattern, connect_until_node, HOST, PORT,
    MENU_SENTINEL, PAUSE_SENTINEL, read_until_any, emit,
)
from bbsdrive import render  # noqa: E402

MORE = b"uit:"            # More? (Y/n/ns), ... (Q)uit:
FLAG = b"to flag:"        # File name(s) to flag:
DIRS = b"=None ?"         # Directories: ... (Enter)=None ?
NSCONF = b"sure "         # Non-stop scrolling! Are you sure (Y/n)?
DATE = b" ?\x1b[0m "      # Date: ... (Enter)=MM-DD-YY ?<ESC>[0m<SP>  (generic)

DEFAULTS = {MORE: b"Y", FLAG: b"\r", DIRS: b"\r", NSCONF: b"n", DATE: b"\r"}


def scenario(bbs, label, send, more=(), flag=(), dirs=(), nsconf=(), date=(),
             maxrounds=80, maxwait=75):
    """Send `send`, then answer every known sub-prompt from per-kind queues
    (safe defaults when a queue runs dry) until the menu prompt returns.
    Recovers to the menu (Q, CR) on surprise so scenarios cannot cascade."""
    queues = {MORE: list(more), FLAG: list(flag), DIRS: list(dirs),
              NSCONF: list(nsconf), DATE: list(date)}
    bbs.send(send)
    collected = b""
    status = "RAN OUT OF ROUNDS"
    for _ in range(maxrounds):
        clean, hit = read_until_any(
            bbs, [MENU_SENTINEL, FLAG, DIRS, NSCONF, MORE, DATE,
                  PAUSE_SENTINEL],
            maxwait=maxwait)
        collected += clean
        if hit == MENU_SENTINEL:
            status = "MENU"
            break
        if hit == PAUSE_SENTINEL:
            collected += b"<<<harness: SPACE>>>"
            bbs.send(b" ")
            continue
        if hit is None:
            status = "TIMEOUT (no sentinel)"
            break
        q = queues.get(hit)
        ans = q.pop(0) if q else DEFAULTS[hit]
        collected += b"<<<harness answers %s: %s>>>" % (hit, ans)
        bbs.send(ans)
    if status != "MENU":
        for esc in (b"Q", b"\r", b"q\r"):
            bbs.send(esc)
            clean, hit = read_until_any(bbs, [MENU_SENTINEL], maxwait=25)
            collected += b"<<<recovery %s>>>" % esc + clean
            if hit == MENU_SENTINEL:
                status += " +RECOVERED"
                break
    emit(label, send, collected, status)
    return collected


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/ae_tierd_newfiles.txt"
    bbs, banner = connect_until_node(HOST, PORT, log=LOG)
    try:
        to_pattern(bbs, "graphics -> A", b"A\r", b"Name:", maxwait=60)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assword", maxwait=40)
        to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=120)
        to_menu(bbs, "ensure conf 2 (Amiga, seeded)", b"J 2\r")

        # ---- the advertised help screen (grammar reference) ----
        scenario(bbs, "N6: N ? (options/help screen)", b"N ?\r")

        # ---- bare-N prompt paths ----
        # N1a: Enter at BOTH prompts — the dirs Enter=None silent abort.
        scenario(bbs, "N1a: bare N -> Enter date -> Enter dirs (None abort)",
                 b"N\r", date=[b"\r"], dirs=[b"\r"])
        # N1b: default date (day of last call), upload dir by number.
        scenario(bbs, "N1b: bare N -> Enter date -> dir 2",
                 b"N\r", date=[b"\r"], dirs=[b"2\r"])
        # N1c: default date across all dirs.
        scenario(bbs, "N1c: bare N -> Enter date -> A (all dirs)",
                 b"N\r", date=[b"\r"], dirs=[b"A\r"])

        # N2: broad date + all dirs, page to the natural end — the money
        # capture (28-entry dir 1 + 3-entry dir 2; default More? answer Y).
        scenario(bbs, "N2: bare N -> 01-01-26 -> A, page to end",
                 b"N\r", date=[b"01-01-26\r"], dirs=[b"A\r"])

        # N3: days form — which day does -30 compute from (today vs last
        # call)? Scan dir 1 so the header date is visible with matches.
        scenario(bbs, "N3: bare N -> -30 -> dir 1",
                 b"N\r", date=[b"-30\r"], dirs=[b"1\r"], more=[b"Y", b"Q"])

        # N4: R alone at the date prompt — reverse semantics after prompts.
        scenario(bbs, "N4: bare N -> R -> dir 2 (reverse)",
                 b"N\r", date=[b"R\r"], dirs=[b"2\r"])
        # N4b: `R <date>` combined at the date prompt — accepted in run 1
        # (went on to the dirs prompt); what does it actually scan?
        scenario(bbs, "N4b: bare N -> R 01-01-26 -> dir 2",
                 b"N\r", date=[b"R 01-01-26\r"], dirs=[b"2\r"])

        # N5: junk date — Error in date! envelope + exit tail.
        scenario(bbs, "N5: bare N -> FOO (junk date)",
                 b"N\r", date=[b"FOO\r"])

        # N8: future date, all dirs — the all-Nothing-found tail.
        scenario(bbs, "N8: bare N -> 12-30-26 -> A (matches nothing)",
                 b"N\r", date=[b"12-30-26\r"], dirs=[b"A\r"])
        # N8b: out-of-range dir number at the dirs prompt.
        scenario(bbs, "N8b: bare N -> Enter date -> dir 9 (out of range)",
                 b"N\r", date=[b"\r"], dirs=[b"9\r"])

        # ---- inline argument forms (no prompts) ----
        scenario(bbs, "N7a: N 01-01-26 (inline date, default dir=upload)",
                 b"N 01-01-26\r")
        scenario(bbs, "N7b: N -30 (inline days)", b"N -30\r")
        scenario(bbs, "N7c: N 01-01-26 1 (inline date + dir, quit mid-list)",
                 b"N 01-01-26 1\r", more=[b"Y", b"Q"])
        scenario(bbs, "N7q: N 01-01-26 2 Q (quick scan: first line only)",
                 b"N 01-01-26 2 Q\r")
        scenario(bbs, "N7ns: N 01-01-26 2 NS (non-stop scroll)",
                 b"N 01-01-26 2 NS\r")
        scenario(bbs, "N7n: N !2 (the 2 newest files)", b"N !2\r")
        scenario(bbs, "N7t: N T (today)", b"N T\r")
        scenario(bbs, "N7y: N Y (yesterday)", b"N Y\r")
        scenario(bbs, "N7s: N S (explicit since-last-call)", b"N S\r")
        scenario(bbs, "N7d: N 2 (bare dir token)", b"N 2\r")
        scenario(bbs, "N7r: N R 2 (inline reverse, dir 2)", b"N R 2\r")
        scenario(bbs, "N7e: N R -1 (invalid combo -> Argument error!)",
                 b"N R -1\r")

        # ---- empty-conference shape (conf 1, Dir1 empty) ----
        to_menu(bbs, "N9: J 1 (empty Dir1 conf)", b"J 1\r")
        scenario(bbs, "N9: bare N -> Enter date -> dir 1 (empty dir)",
                 b"N\r", date=[b"\r"], dirs=[b"1\r"])
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
