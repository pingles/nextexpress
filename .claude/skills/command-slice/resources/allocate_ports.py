#!/usr/bin/env python3
"""Allocate per-run TCP ports for a command-slice run, and flag resources we must not corrupt.

Picks a free host port to map the FS-UAE board's telnet (container :6023) and a free
port for the NextExpress server. Both are derived from the worktree path so parallel
worktrees tend to disjoint bands, then advanced to the next actually-free port so two
runs never collide.

This run boots its OWN board container; it must never reuse or kill another session's
board (that corrupts their state — see resources/board-lifecycle.md). So the tool only
*reports* any running reference container and any stale `nextexpress` listener on a
candidate port; it never touches them.

Output: a JSON object on stdout, e.g.
  {"worktree": "...", "board_port": 34871, "server_port": 34872,
   "board_containers": [], "stale_servers": [], "warnings": []}

Exit status: 0 when both ports were allocated, 2 if no free port could be found.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import socket
import subprocess
import sys

BAND_START = 20000  # above privileged + most common dev ports
BAND_SIZE = 20000   # search window [BAND_START, BAND_START + BAND_SIZE)


def worktree_base(worktree: str) -> int:
    """Return a stable starting port derived from the worktree path.

    Distinct worktrees hash to distinct offsets, so parallel runs start their search in
    different places and rarely contend for the same port.

    :param worktree: absolute or relative worktree path.
    :returns: a port in ``[BAND_START, BAND_START + BAND_SIZE)``.
    """
    digest = hashlib.sha256(worktree.encode()).digest()
    return BAND_START + int.from_bytes(digest[:4], "big") % BAND_SIZE


def port_free(port: int) -> bool:
    """Return True if ``127.0.0.1:port`` can be bound right now.

    ``SO_REUSEADDR`` is left off so a port that is in use (or lingering in TIME_WAIT)
    reports as not free — a deliberately strict check.

    :param port: TCP port to test.
    :returns: whether the port is currently bindable.
    """
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        try:
            sock.bind(("127.0.0.1", port))
            return True
        except OSError:
            return False


def next_free(start: int, taken: set) -> "int | None":
    """Find the next free port at or after ``start``, wrapping within the band.

    :param start: port to begin searching from.
    :param taken: ports already claimed by this run (skipped even if bindable).
    :returns: a free port, or ``None`` if the whole band is exhausted.
    """
    end = BAND_START + BAND_SIZE
    for port in list(range(start, end)) + list(range(BAND_START, start)):
        if port not in taken and port_free(port):
            return port
    return None


def running_board_containers() -> list:
    """Return names of running containers that look like an AmiExpress reference board.

    :returns: container names (empty if docker is absent or none match).
    """
    if not shutil.which("docker"):
        return []
    try:
        result = subprocess.run(
            ["docker", "ps", "--format", "{{.Names}}\t{{.Image}}"],
            capture_output=True, text=True, timeout=10, check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return []
    names = []
    for line in result.stdout.splitlines():
        name, _, image = line.partition("\t")
        if "amiexpress" in name.lower() or "amiexpress" in image.lower():
            names.append(name)
    return names


def stale_servers(ports: list) -> list:
    """Best-effort report of any ``nextexpress`` process listening on a candidate port.

    :param ports: candidate ports to inspect.
    :returns: human-readable descriptions of stale listeners (empty if lsof is absent).
    """
    if not shutil.which("lsof"):
        return []
    hits = []
    for port in ports:
        try:
            result = subprocess.run(
                ["lsof", "-nP", f"-iTCP:{port}", "-sTCP:LISTEN"],
                capture_output=True, text=True, timeout=10, check=False,
            )
        except (OSError, subprocess.SubprocessError):
            continue
        if "nextexpress" in result.stdout.lower():
            hits.append(f"port {port}: {result.stdout.strip().splitlines()[-1]}")
    return hits


def allocate(worktree: str) -> dict:
    """Allocate the board and server ports for a run and gather non-corruption warnings.

    :param worktree: worktree path the ports are derived from.
    :returns: the result dict (see module docstring); contains ``"error"`` if allocation failed.
    """
    base = worktree_base(worktree)
    board_port = next_free(base, taken=set())
    if board_port is None:
        return {"error": "no free port for the board"}
    server_port = next_free(board_port + 1, taken={board_port})
    if server_port is None:
        return {"error": "no free port for the server"}

    warnings = []
    boards = running_board_containers()
    if boards:
        warnings.append(
            f"running reference board(s) present ({', '.join(boards)}); do NOT reuse or "
            "kill them — this run boots its own container")
    stale = stale_servers([board_port, server_port])
    if stale:
        warnings.append("stale nextexpress listener(s) on candidate ports: " + "; ".join(stale))

    return {
        "worktree": worktree,
        "board_port": board_port,
        "server_port": server_port,
        "board_containers": boards,
        "stale_servers": stale,
        "warnings": warnings,
    }


def main(argv=None) -> int:
    """CLI entry point: allocate ports for ``--worktree`` (default cwd) and print JSON."""
    parser = argparse.ArgumentParser(description="Allocate per-run ports for a command-slice run.")
    parser.add_argument("--worktree", default=os.getcwd(),
                        help="worktree path the ports are derived from (default: current directory)")
    args = parser.parse_args(argv)

    result = allocate(args.worktree)
    json.dump(result, sys.stdout, indent=2)
    print()
    return 2 if "error" in result else 0


if __name__ == "__main__":
    raise SystemExit(main())
