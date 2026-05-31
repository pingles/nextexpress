# Live NextExpress vs AmiExpress comparison

Supporting evidence for [`../COMMAND_PARITY.md`](../COMMAND_PARITY.md) — a per-command
behavioural + wire-format comparison of NextExpress (the Rust port) against the
genuine **AmiExpress 5.6.0** binary, both exercised **live over telnet**.

- NextExpress: `./nextexpress nextexpress.toml` (port 2323).
- AmiExpress 5.6.0: the `docker/amiexpress-fsuae` FS-UAE harness (port 6023),
  run with `NODE_COUNT=4` (multi-node) and `DOSCHECKTIME=0` (DoS throttle off
  for localhost testing).

## `transcripts/` — raw captured wire bytes

Each capture is segmented per command (`@@@@@ <label> @@@@@`) with a
human-readable RENDER (control/ANSI bytes made visible) and a raw `repr()`.

| File | What it is |
|---|---|
| `rust_sysop.txt` | NextExpress full command battery (login → every command → logoff). |
| `amiexpress_sysop.txt` | AmiExpress full command battery. |
| `amiexpress_login.txt` | AmiExpress login + the full ANSI menu screen. |
| `amiexpress_post_and_list.txt` | AmiExpress **posting a real message** through its line editor + the message list. |
| `amiexpress_read_messages.txt` | AmiExpress **reading** that message back (live header + body). |

## `harness/` — the telnet drivers used to capture the above

- `bbsdrive.py` — telnet driver core (`read_idle` / `read_until` / IAC stripping / RENDER).
- `ae_session.py` — AmiExpress session helper (`connect_until_node`; every session
  ends with a clean `G Y` logoff because an abrupt close spins a node forever
  under FS-UAE's bsdsocket emulation).
- `rust_capture.py`, `ae_battery.py` — the command batteries.
- `ae_content.py`, `ae_read2.py` — post-a-message and read-it-back content captures.

> The two installs carry different seeded data (conferences, message counts,
> clocks, board name), so the comparison is about **behaviour and wire format**,
> not data values. See `../COMMAND_PARITY.md` for the analysis and the
> cosmetic-vs-behavioural classification of every difference.
