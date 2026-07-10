#!/usr/bin/env python3
"""One-session D10 completeness re-probe after the four-open recycle.

Closes only the gaps named by the Stage-2 completeness critic:

* numeric/name whitespace, bare-LF and multi-token line grammar;
* reload after the current catalogue changes;
* numeric selection from a populated HOLD catalogue.

The script changes only the isolated reference container.  Dir2 and Hold/Held
are restored from checked-in/the pre-probe fixtures in ``finally`` before the
clean ``G Y`` logoff.  Pager behaviour already grounded by D2 and N prompt
behaviour grounded by D9 are deliberately not re-probed.
"""

import os
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from ae_session import connect_until_node  # noqa: E402
from ae_tierc import (  # noqa: E402
    LOG,
    MENU_SENTINEL,
    PAUSE_SENTINEL,
    emit,
    read_until_any,
    to_menu,
    to_pattern,
)
from ae_tierd_d10_selection import (  # noqa: E402
    FLAG_NAME,
    FLAG_NUMBER,
    MORE,
    marker,
    show_and_clear_flags,
)
from bbsdrive import render  # noqa: E402


HOST = os.environ.get("AE_HOST", "127.0.0.1")
PORT = int(os.environ.get("AE_PORT", "30569"))
CONTAINER = os.environ.get(
    "AE_CONTAINER", "nextexpress-ref-nextscan-index"
)

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "comparison" / "evidence-tierD" / "fixtures"
EMPTY_HOLD = Path("/private/tmp/nextscan-d10-held.backup")
DIR2_DEST = f"{CONTAINER}:/amiga/bbs/Conf02/Dir2"
HOLD_DEST = f"{CONTAINER}:/amiga/bbs/Conf02/Hold/Held"


def docker_copy(source, destination):
    """Copy one fixture, failing the capture rather than hiding drift."""
    subprocess.run(
        ["docker", "cp", str(source), str(destination)],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )


def send_marked(bbs, collected, kind, payload):
    """Record a harness action beside the bytes it causes."""
    collected += marker(kind, payload)
    bbs.send(payload)
    return collected


def wait_for(bbs, collected, expected, *, maxwait=75):
    """Read until ``expected``, draining ordinary Pause gates safely."""
    prompts = [expected, MENU_SENTINEL, PAUSE_SENTINEL]
    for _ in range(20):
        clean, hit = read_until_any(bbs, prompts, maxwait=maxwait)
        collected += clean
        if hit == PAUSE_SENTINEL:
            collected = send_marked(bbs, collected, b"Pause", b" ")
            continue
        if hit != expected:
            raise RuntimeError(f"expected {expected!r}, got {hit!r}")
        return collected
    raise RuntimeError(f"did not converge on {expected!r}")


def start_to_more(bbs, command):
    """Start one short listing and stop at its first More? gate."""
    bbs.send(command)
    collected = b""
    return wait_for(bbs, collected, MORE, maxwait=120)


def open_flag_prompt(bbs, collected, verb, expected):
    """Choose F/R at More? and stop after the line prompt appears."""
    collected = send_marked(bbs, collected, b"More", verb)
    return wait_for(bbs, collected, expected)


def submit_line(bbs, collected, kind, payload):
    """Submit a CR-terminated flag line and return at More?."""
    collected = send_marked(bbs, collected, kind, payload)
    return wait_for(bbs, collected, MORE)


def submit_bare_lf_then_cancel(bbs, collected, kind):
    """Probe LF alone; if it is swallowed, CR-cancel the still-open line."""
    collected = send_marked(bbs, collected, kind, b"\n")
    idle = bbs.read_idle(idle=1.5, maxwait=4)
    collected += marker(kind + b" idle bytes", idle) + idle
    if MORE not in idle:
        collected = send_marked(bbs, collected, kind + b" recovery", b"\r")
        collected = wait_for(bbs, collected, MORE)
    return collected


def submit_prefix_then_rest(bbs, collected, kind, prefix, rest):
    """Show that a partial line echoes but does not submit before CR."""
    collected = send_marked(bbs, collected, kind + b" prefix", prefix)
    idle = bbs.read_idle(idle=1.5, maxwait=4)
    collected += marker(kind + b" prefix idle bytes", idle) + idle
    if MORE in idle:
        raise RuntimeError("line prompt accepted a prefix without CR")
    collected = send_marked(bbs, collected, kind + b" rest", rest)
    return wait_for(bbs, collected, MORE)


def finish_pager(bbs, collected):
    """Quit the door and drain its Pause gate back to a clean menu."""
    collected = send_marked(bbs, collected, b"More", b"Q")
    return wait_for(bbs, collected, MENU_SENTINEL, maxwait=120)


def recover_to_menu(bbs):
    """Best-effort bounded recovery after a failed pager/sub-prompt step."""
    prompts = [
        MENU_SENTINEL,
        PAUSE_SENTINEL,
        MORE,
        FLAG_NUMBER,
        FLAG_NAME,
    ]
    for attempt in range(12):
        clean, hit = read_until_any(bbs, prompts, maxwait=12)
        LOG.append(f"\n@@@@@ D10 RECOVERY {attempt} hit={hit!r} @@@@@")
        LOG.append(render(clean))
        if hit == MENU_SENTINEL:
            return True
        if hit == PAUSE_SENTINEL:
            bbs.send(b" ")
        elif hit == MORE:
            bbs.send(b"Q")
        elif hit in (FLAG_NUMBER, FLAG_NAME):
            bbs.send(b"\r")
        else:
            # Unknown line-read state: CR first; the next iteration can
            # recognize and quit a redrawn More? prompt.
            bbs.send(b"\r")
    return False


def numeric_edges(bbs):
    collected = start_to_more(bbs, b"F 2\r")

    collected = open_flag_prompt(bbs, collected, b"R", FLAG_NUMBER)
    collected = submit_line(bbs, collected, b"numeric whitespace", b"   \r")

    collected = open_flag_prompt(bbs, collected, b"R", FLAG_NUMBER)
    collected = submit_bare_lf_then_cancel(bbs, collected, b"numeric bare LF")

    collected = open_flag_prompt(bbs, collected, b"R", FLAG_NUMBER)
    collected = submit_prefix_then_rest(
        bbs,
        collected,
        b"numeric plural duplicate",
        b"1",
        b" 2 1\r",
    )

    collected = finish_pager(bbs, collected)
    emit(
        "D10-E1: numeric whitespace/LF/line-read/plural duplicate",
        b"F 2\r",
        collected,
        "MENU",
    )
    show_and_clear_flags(bbs, "D10-E1A: internal A verifies numeric set")


def name_edges(bbs):
    collected = start_to_more(bbs, b"F 2\r")

    collected = open_flag_prompt(bbs, collected, b"F", FLAG_NAME)
    collected = submit_line(bbs, collected, b"name whitespace", b"   \r")

    collected = open_flag_prompt(bbs, collected, b"F", FLAG_NAME)
    collected = submit_bare_lf_then_cancel(bbs, collected, b"name bare LF")

    collected = open_flag_prompt(bbs, collected, b"F", FLAG_NAME)
    collected = submit_prefix_then_rest(
        bbs,
        collected,
        b"name plural duplicate",
        b"freshupl.lha",
        b" mydemo.dms freshupl.lha\r",
    )

    collected = finish_pager(bbs, collected)
    emit(
        "D10-E2: name whitespace/LF/line-read/plural duplicate",
        b"F 2\r",
        collected,
        "MENU",
    )
    show_and_clear_flags(bbs, "D10-E2A: internal A verifies name set")


def changed_reload(bbs):
    collected = start_to_more(bbs, b"F 2\r")
    docker_copy(FIXTURES / "Dir1", DIR2_DEST)
    collected += marker(b"fixture mutation", b"Dir2 <- Dir1")
    collected = send_marked(bbs, collected, b"More", b"L")
    collected = wait_for(bbs, collected, MORE, maxwait=120)
    collected = open_flag_prompt(bbs, collected, b"R", FLAG_NUMBER)
    collected = submit_line(bbs, collected, b"post-reload number", b"1\r")
    collected = finish_pager(bbs, collected)
    docker_copy(FIXTURES / "Dir2", DIR2_DEST)
    collected += marker(b"fixture restore", b"Dir2 <- committed Dir2")
    emit(
        "D10-E3: changed Dir2 reload then R 1",
        b"F 2\r",
        collected,
        "MENU; DIR2 RESTORED",
    )
    show_and_clear_flags(bbs, "D10-E3A: internal A verifies reloaded R 1")


def populated_hold(bbs):
    collected = start_to_more(bbs, b"F H\r")
    collected = open_flag_prompt(bbs, collected, b"R", FLAG_NUMBER)
    collected = submit_line(bbs, collected, b"hold number", b"1\r")
    collected = finish_pager(bbs, collected)
    docker_copy(EMPTY_HOLD, HOLD_DEST)
    collected += marker(b"fixture restore", b"Hold/Held <- empty backup")
    emit(
        "D10-E4: populated HOLD then R 1",
        b"F H\r",
        collected,
        "MENU; HOLD RESTORED",
    )
    show_and_clear_flags(bbs, "D10-E4A: internal A verifies HOLD R 1")


def main():
    out_path = (
        sys.argv[1]
        if len(sys.argv) > 1
        else "comparison/transcripts/ae_tierd_d10_edge_reprobe.txt"
    )

    bbs = None
    try:
        bbs, _banner = connect_until_node(
            HOST, PORT, retries=8, delay=4.0, log=LOG
        )
        to_pattern(bbs, "graphics -> A", b"A\r", b"Name:", maxwait=60)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assword", maxwait=40)
        to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=120)
        to_menu(bbs, "ensure conf 2 (Amiga, seeded)", b"J 2\r")
        show_and_clear_flags(bbs, "D10-E0: initial flag cleanup")

        numeric_edges(bbs)
        name_edges(bbs)
        changed_reload(bbs)
        populated_hold(bbs)
        to_menu(bbs, "restore rejoin target: J 2", b"J 2\r")
    finally:
        # Fixture restoration is unconditional, including driver failures.
        docker_copy(FIXTURES / "Dir2", DIR2_DEST)
        docker_copy(EMPTY_HOLD, HOLD_DEST)
        if bbs is not None:
            at_menu = False
            try:
                at_menu = recover_to_menu(bbs)
                if at_menu:
                    bbs.send(b"G Y\r")
                    for _ in range(8):
                        clean = bbs.read_idle(idle=1.5, maxwait=12)
                        LOG.append("\n@@@@@ CLEAN G Y LOGOFF round @@@@@")
                        LOG.append(render(clean))
                        if clean == b"":
                            break
                        low = clean.lower()
                        if b"y/n" in low or b"sure" in low:
                            bbs.send(b"Y\r")
                else:
                    LOG.append(
                        "\n@@@@@ RECOVERY FAILED; CONTAINER RECYCLE REQUIRED @@@@@"
                    )
            except OSError:
                pass
            bbs.close()
            if not at_menu:
                subprocess.run(
                    ["docker", "restart", CONTAINER],
                    check=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.STDOUT,
                )
        with open(out_path, "w", encoding="utf-8") as handle:
            handle.write("\n".join(LOG))
        print(f"wrote {out_path}")


if __name__ == "__main__":
    main()
