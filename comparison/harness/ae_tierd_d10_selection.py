#!/usr/bin/env python3
"""D10 capture — AquaScan's listed-file selection registry.

The board-as-shipped `F`/`FR`/`N` tokens are AquaScan v1.0 door icons, not
`internalCommandF` (`express.e:28229-28256` dispatches command icons before
`processInternalCommand` at :28285).  The operator selected this door facet as
the D10 authority (Stage-1 gate A/A, 2026-07-10).

This one-session capture focuses on the registry behaviours D10 changes:

* `F A`: after directory 2 has emitted its own `[ File #1 ]`, `R 1`;
* `FR A`: the equivalent reverse walk, selecting after directory 2 emits #1;
* `N` with a date that makes dir 1 empty and dir 2 populated;
* `L` reload followed by empty/unknown/trailing numeric and name forms;
* `F H`, recording whether the live fixture exposes any hold rows.

After each mutating pager probe, genuine internal `A` (`alterFlags`,
express.e:12648) records the resulting names and clears them.  The fixture has
no duplicate normalized names across Dir1/Dir2; that limitation is recorded in
the evidence note instead of manufacturing a result.

Reference access is serialized.  The authoritative run uses one telnet open;
the complete capture campaign used four opens including three discarded setup /
driver diagnostics (<5 budget).  Every run ended with a clean `G Y` logoff
(FS-UAE node-spin hazard).
"""

import os
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
from bbsdrive import render  # noqa: E402


HOST = os.environ.get("AE_HOST", "127.0.0.1")
PORT = int(os.environ.get("AE_PORT", "30569"))

MORE = b"uit:"                 # AquaScan ... (Q)uit:
FLAG_NAME = b"name(s) to flag:"
FLAG_NUMBER = b"number(s) to flag:"
ALTER_FLAG = b"Filename(s) to flag:"
CLEAR_FLAG = b"Filename(s) to Clear:"
DIRS = b"=None ?"
DATE = b" ?\x1b[0m "
NS_CONFIRM = b"sure "


def marker(kind, answer):
    return b"<<<D10 harness answers %s: %s>>>" % (kind, answer)


def pager_scenario(
    bbs,
    label,
    command,
    more_answer,
    *,
    number_answers=(),
    name_answers=(),
    date_answers=(),
    dir_answers=(),
    max_rounds=100,
    max_wait=75,
):
    """Drive one AquaScan listing with an adaptive More? callback.

    `more_answer(index, latest_chunk, collected)` returns the next hotkey.
    Every line-read prompt has a bounded queue.  Exhausting a queue uses CR,
    which is the safe empty selection.  Any unexpected timeout is recovered
    with `Q`/CR and recorded, never allowed to cascade into another scenario.
    """
    queues = {
        FLAG_NUMBER: list(number_answers),
        FLAG_NAME: list(name_answers),
        DATE: list(date_answers),
        DIRS: list(dir_answers),
        NS_CONFIRM: [b"n"],
    }
    prompts = [
        MENU_SENTINEL,
        FLAG_NUMBER,
        FLAG_NAME,
        DIRS,
        DATE,
        NS_CONFIRM,
        MORE,
        PAUSE_SENTINEL,
    ]
    bbs.send(command)
    collected = b""
    status = "RAN OUT OF ROUNDS"
    more_index = 0

    for _ in range(max_rounds):
        clean, hit = read_until_any(bbs, prompts, maxwait=max_wait)
        collected += clean
        if hit == MENU_SENTINEL:
            status = "MENU"
            break
        if hit == PAUSE_SENTINEL:
            collected += marker(b"Pause", b"SPACE")
            bbs.send(b" ")
            continue
        if hit == MORE:
            answer = more_answer(more_index, clean, collected)
            more_index += 1
            collected += marker(b"More", answer)
            bbs.send(answer)
            continue
        if hit in queues:
            queue = queues[hit]
            answer = queue.pop(0) if queue else b"\r"
            collected += marker(hit, answer)
            bbs.send(answer)
            continue
        status = "TIMEOUT (no sentinel)"
        break

    if status != "MENU":
        for escape in (b"Q", b"\r", b"q\r"):
            bbs.send(escape)
            clean, hit = read_until_any(bbs, [MENU_SENTINEL], maxwait=25)
            collected += b"<<<D10 recovery %r>>>" % escape + clean
            if hit == MENU_SENTINEL:
                status += " +RECOVERED"
                break

    emit(label, command, collected, status)
    return collected


def at_dir2_file_one():
    """Return a callback that selects R 1 only after dir 2 emitted #1."""
    selected = False

    def answer(_index, latest, collected):
        nonlocal selected
        folded = collected.lower()
        # Covers both `Scanning dir 2` and `Reverse-scanning dir 2`.
        dir2 = folded.rfind(b"scanning dir 2")
        file_one = folded.rfind(b"file #1")
        if not selected and dir2 >= 0 and file_one > dir2:
            selected = True
            return b"R"
        if selected:
            return b"Q"
        return b"Y"

    return answer


def scripted_more(*answers):
    """Return a bounded More? script; default to Q when exhausted."""
    answers = list(answers)

    def answer(index, _latest, _collected):
        return answers[index] if index < len(answers) else b"Q"

    return answer


def show_and_clear_flags(bbs, label):
    """Capture internal `A`, then `C *`, leaving the persisted set empty."""
    collected = b""

    bbs.send(b"A\r")
    clean, hit = read_until_any(
        bbs, [ALTER_FLAG, MENU_SENTINEL], maxwait=45
    )
    collected += clean
    if hit != ALTER_FLAG:
        emit(label, b"A\r", collected, f"EXPECTED ALTER PROMPT, GOT {hit!r}")
        return

    collected += marker(b"alterFlags", b"C\r")
    bbs.send(b"C\r")
    clean, hit = read_until_any(
        bbs, [CLEAR_FLAG, ALTER_FLAG, MENU_SENTINEL], maxwait=30
    )
    collected += clean
    if hit == CLEAR_FLAG:
        collected += marker(b"clear", b"*\r")
        bbs.send(b"*\r")
        clean, hit = read_until_any(
            bbs, [ALTER_FLAG, MENU_SENTINEL], maxwait=30
        )
        collected += clean
    if hit == ALTER_FLAG:
        collected += marker(b"alterFlags exit", b"CR")
        bbs.send(b"\r")
        # alterFlags exits through the ordinary menu redisplay, which can
        # itself page at `(Pause)...Space To Resume:`.  Drain that gate here;
        # otherwise the next scenario's command would be swallowed by it.
        for _ in range(8):
            clean, hit = read_until_any(
                bbs, [MENU_SENTINEL, PAUSE_SENTINEL], maxwait=45
            )
            collected += clean
            if hit == MENU_SENTINEL:
                break
            if hit == PAUSE_SENTINEL:
                collected += marker(b"menu Pause", b"SPACE")
                bbs.send(b" ")
                continue
            break

    status = "MENU; FLAGS CLEARED" if hit == MENU_SENTINEL else f"GOT {hit!r}"
    emit(label, b"A\r", collected, status)


def main():
    out_path = (
        sys.argv[1]
        if len(sys.argv) > 1
        else "comparison/transcripts/ae_tierd_d10_selection.txt"
    )

    bbs, _banner = connect_until_node(HOST, PORT, retries=50, delay=4.0, log=LOG)
    try:
        to_pattern(bbs, "graphics -> A", b"A\r", b"Name:", maxwait=60)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assword", maxwait=40)
        to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=120)
        to_menu(bbs, "ensure conf 2 (Amiga, seeded)", b"J 2\r")

        # Start from deterministic empty flags, irrespective of prior captures.
        show_and_clear_flags(bbs, "D10-0: initial flag state and cleanup")

        pager_scenario(
            bbs,
            "D10-1: F A, dir 2 File #1 -> R 1",
            b"F A\r",
            at_dir2_file_one(),
            number_answers=[b"1\r"],
        )
        show_and_clear_flags(
            bbs, "D10-1A: internal A verifies forward cross-dir R 1"
        )

        pager_scenario(
            bbs,
            "D10-2: FR A, dir 2 reverse File #1 -> R 1",
            b"FR A\r",
            at_dir2_file_one(),
            number_answers=[b"1\r"],
        )
        show_and_clear_flags(
            bbs, "D10-2A: internal A verifies reverse cross-dir R 1"
        )

        # 06-10-26 leaves seeded dir 1 empty for N while dir 2 has rows.
        pager_scenario(
            bbs,
            "D10-3: N empty-dir transition, dir 2 File #1 -> R 1",
            b"N\r",
            at_dir2_file_one(),
            date_answers=[b"06-10-26\r"],
            dir_answers=[b"A\r"],
            number_answers=[b"1\r"],
        )
        show_and_clear_flags(
            bbs, "D10-3A: internal A verifies N transition R 1"
        )

        # Final More? on F 2: reload; then probe empty, out-of-range,
        # non-numeric and trailing-junk numeric forms, followed by empty,
        # unknown and lower-case/trailing-space name forms.  Valid forms are
        # last in each group so the final A listing disambiguates failures.
        pager_scenario(
            bbs,
            "D10-4: F 2 reload then numeric/name grammar",
            b"F 2\r",
            scripted_more(b"L", b"r", b"R", b"R", b"R", b"F", b"f", b"F", b"Q"),
            number_answers=[b"\r", b"999\r", b"abc\r", b"1 garbage\r"],
            name_answers=[b"\r", b"nosuch.lha\r", b"mydemo.dms   \r"],
        )
        show_and_clear_flags(
            bbs, "D10-4A: internal A verifies reload/grammar selections"
        )

        # The seeded live HOLD catalogue has historically been empty.  If it
        # remains empty there is no pager on which F/R can be attempted; the
        # transcript pins that gate and the evidence note marks row selection
        # uncapturable from this fixture rather than extrapolating door code.
        pager_scenario(
            bbs,
            "D10-5: F H hold-row gate (F/R only if pager exists)",
            b"F H\r",
            scripted_more(b"R", b"F", b"Q"),
            number_answers=[b"1\r"],
            name_answers=[b"STARVIEW.LHA\r"],
        )
        show_and_clear_flags(
            bbs, "D10-5A: internal A verifies any HOLD F/R effects"
        )

        to_menu(bbs, "restore rejoin target: J 2", b"J 2\r")
    finally:
        try:
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
        except OSError:
            pass
        bbs.close()
        with open(out_path, "w", encoding="utf-8") as handle:
            handle.write("\n".join(LOG))
        print(f"wrote {out_path}")


if __name__ == "__main__":
    main()
