# board-lifecycle.md — booting, driving, tearing down the two boards

Operator runbook for one command-slice run: bring up **our own** FS-UAE reference
board and the **NextExpress** server, drive them safely, and tear both down clean.
Distilled from the `amiexpress-docker-harness` / `tierd-aquascan-parity-target`
auto-memory and the real drivers in `comparison/harness/` (`bbsdrive.py`,
`ae_session.py`, `ae_tierc.py`, `ae_tierd_newfiles.py`). Bound by §10.5 (symmetric
server + board lifecycle, resumable) and §10.8 (connection budget, bounded loops).

Golden rule: **one board per run, reference access serialized, every session ends
`G Y`.** Never touch another session's board.

---

## (a) Ports — `allocate_ports.py`

```sh
python resources/allocate_ports.py --worktree "$PWD"
```

Emits JSON: `board_port`, `server_port` (both free, worktree-derived so parallel
worktrees disjoint), plus `board_containers` / `stale_servers` / `warnings`.
Exit 2 = no free port. It only **reports** existing boards/servers — it never
kills them (§10.5). Record both ports in the run-state file (see (f)).

- `board_port` → host side of the FS-UAE telnet map (`127.0.0.1:board_port->6023`).
- `server_port` → NextExpress `port` key.

---

## (b) Clean-state check + boot our own container

**Detect, never adopt.** If `board_containers` is non-empty, that is *another
session's* board. Do **not** `docker kill`/`rm`/reuse it — that corrupts their
run. Boot a per-run container with a unique name and our own host port:

```sh
docker run -d --name nextexpress-ref-<run> \
  -p 127.0.0.1:<board_port>:6023 \
  -e NODE_COUNT=4 -e DOSCHECKTIME=0 \
  -v nextexpress-aros-roms:/opt/aros \
  -v nextexpress-aros-system:/amiga/workbench \
  -v nextexpress-bbs:/amiga/bbs \
  nextexpress/amiexpress-fsuae:latest
```

- `<run>` = short unique token (worktree slug / PID) so parallel runs never collide
  on the container name.
- Emulated boot to a live telnet listener takes **~2–3 min**. Boot **early during
  Stage 1** to hide the latency behind assessment; reuse across Stages 2 and 5.
- `DOSCHECKTIME=0` disables the DoS self-ban (see (d)); the stock entrypoint already
  sets it, pass it explicitly as belt-and-braces.

### Shared-volume corruption hazard — the safe default

All boards share the `nextexpress-bbs` volume (`acpConnections.dat`, flag files,
`joinConf` state). **Two live boards on the same volume corrupt each other.** Safe
default (what this skill does): **one board per run, reference-side access
serialized** through a single controlled session. Truly-parallel runs need *cloned*
volumes — do not run two boards against `nextexpress-bbs` at once.

---

## (c) Login + driving protocol

Login flow (matches `ae_session.py::login`, `ae_tierd_newfiles.py::main`):

| Prompt (pattern to match) | Send | Note |
|---|---|---|
| `ANSI, RIP or No graphics (A/r/n)?` (`graphics`) | `A\r` | `lineInput` — needs the CR; **bare `A` hangs forever** |
| `Enter your Name:` (`Name:`) | `sysop\r` | seeded account |
| `Password:` (`assword`) | `sysop\r` | sysop/sysop, sec 255, auto-rejoins conf 2 "Amiga" |

Then read to the menu prompt.

**Driving protocol — pattern-match prompts, NEVER fixed sleeps.** Per-prompt
latency is high and variable (emulated m68k, ~120% idle CPU with 4 nodes). Use
expect-with-generous-timeout (`read_until`, `read_until_any`), not a fixed cadence.

- Menu resync sentinel: **`mins. left): `** (`MENU_SENTINEL`) — also matches the
  NextExpress prompt suffix, so the same sentinel works both sides.
- Pager gate: `(Pause)...Space To Resume:` (`PAUSE_SENTINEL`) → **auto-answer a
  space** and continue.
- Board-readiness / node grab: retry-connect until the banner contains
  **`Successful connection to node`** *and* `graphics` (`connect_until_node`). A
  connection showing "No nodes available" / refused did **not** grab a node → close
  it safely and retry. Only a node-holding connection must be logged off.
- Sub-prompt auto-answer: keep a per-kind queue with safe defaults, exactly like
  `ae_tierd_newfiles.py::scenario` (`More?`→`Y`, flag→`\r`, dirs→`\r`,
  non-stop→`n`, date→`\r`), and recover to the menu with `Q`/`\r` on any surprise
  so one scenario cannot cascade into the next.

---

## (d) Hard hazards — each with its mitigation

| Hazard | Mitigation |
|---|---|
| **Node spin on unclean close.** An abruptly-closed telnet socket is NOT seen as carrier-loss under FS-UAE `bsdsocket`; the node spin-waits forever, pegging the emulated CPU. | **EVERY session ends `G Y`** (the `Y` bypasses the flagged-file confirm, `amiexpress/express.e:25047-25067`). Drain to EOF / `No Carrier` / `Goodbye` before `close()`. |
| **Same-user two-node block / phantom login.** AmiExpress refuses the same user on two nodes ("already logged into another node!"); an unclean end leaves a phantom login that keeps blocking. | Use **distinct users** for any parallel sessions; restart the container to clear a phantom. Prefer serialized single-session reference access (default). |
| **DoS self-ban behind Docker NAT.** ACP bans an IP after 5 connections; behind NAT every connection is `172.17.0.1`, so any concurrency self-bans. Ban persists in `/amiga/bbs/acpConnections.dat`. | `DOSCHECKTIME=0` at boot; **connection budget < 5 telnet opens before recycling the container** (§10.8). To clear a ban, delete `acpConnections.dat` (or recycle the container). |
| **Capture pollution — `joinConf` persistence.** `joinConf` persists the last-joined conference as the next session's auto-rejoin target (`express.e:5135`); a session logs into wherever the previous one ended. | Never trust positional "from conf N" state; **`J <n>` to a known conference at the start** of each scenario and re-verify per session. Restore the rejoin target before logoff (e.g. `J 2`). |
| **Capture pollution — sub-prompt eats the next line.** Any command that opens a sub-prompt (`Conference Number (1-N):`, `Message Base Number (1-N):`, account editor) consumes the NEXT scripted line as its input — **including a trailing `G Y`**, which then never logs off → node spin. | Always drive to a **clean menu prompt before the next command** and before `G Y`; pattern-match, never assume menu state. |
| **Door-pager eats scripted commands.** AquaScan's `More? (Y/n/ns), (C)lear, (F/R) Flag, (?) Help, (Q)uit:` reads single-key hotkeys (unknown keys continue, `F` opens a line-read flag prompt) and will swallow following menu commands and a final `G Y` (has caused a node spin). | Drive pagers **explicitly per scenario** (see `ae_tierd_newfiles.py::scenario`, `ae_tierd_aquascan.py::drive_pager`); exit to a clean menu prompt before the next command. |

### Door-shadow map (what a token actually runs)

The stock deployment ships **AquaScan v1.0** door icons in `BBS:Commands/BBSCmd/`
that shadow these tokens; `processCommand` (`express.e:28229-28256`) runs door icons
**before** internal commands. So on the board these tokens run the **door**, not the
internal proc:

```
CS   F   FR   N   NS   NSU   SCAN   SENT
```

Every **other** token captures the genuine internal command. To capture a stock
internal for a shadowed token, move the icon into `BBS:Storage/DisabledCmds/` **and
restart** (the disk-object cache keeps serving moved icons). Parity target for
`F`/`FR`/`N` is the **AquaScan door experience** (board-as-shipped), not
`internalCommandF` — see `tierd-aquascan-parity-target` and §10.3 (door-vs-source
conflicts HALT to a human gate, express.e-wins default).

---

## (e) NextExpress server lifecycle

Symmetric to the board (§10.5). Needed for Stage 5.

1. **Point it at `server_port`.** The listener port is the `port` key in
   `nextexpress.toml` (default `2323`). Copy the config to a per-run file with
   `port = <server_port>` (and `bbs_path` at the repo root) — do not clobber the
   checked-in `nextexpress.toml`.
2. **Boot** the built binary with the config path as its first argument:
   `rust/target/release/nextexpress <run-config>.toml` (or
   `cargo run --manifest-path rust/Cargo.toml -- <run-config>.toml`). Record the
   **PID**.
3. **Readiness check:** connect telnet to `127.0.0.1:<server_port>` in a bounded
   retry loop until the login/menu banner appears (the `mins. left): ` sentinel
   works here too) — do not assume it is up after a fixed sleep. Seeded sysop/sysop,
   in-memory adapters.
4. **Teardown:** **kill by recorded PID** on run-end **and on any stage failure**.
   `allocate_ports.py` reports a stale `nextexpress` listener squatting a candidate
   port — investigate/kill it before booting.

The NextExpress side has **no node-spin / DoS hazards** — Tester-A (NextExpress) may
fan out and run in parallel; only the FS-UAE reference side is serialized.

---

## (f) Run-state file + resume reconciliation (§10.5, §10.8)

Persist a run-state record at every stage boundary:

```
stage, scenario_index, board_port, server_port,
container_name (nextexpress-ref-<run>), server_pid
```

**On resume, reconcile live resources FIRST — before continuing the stage:**

1. **Drain + `G Y`** any open board session (never leave a spinning node), or, if
   the session is unrecoverable, recycle the container.
2. **Kill any stale NextExpress server** by the recorded PID (and check
   `allocate_ports.py` `stale_servers`).
3. Only then continue from the recorded stage. **Never double-book a board** —
   a second concurrent login on the same volume/user hits the two-node block / DoS
   self-ban and corrupts the shared `nextexpress-bbs` volume.

Every convergence loop here is **bounded** (§10.8): connect-retry, readiness-check,
and logoff-drain all cap out and escalate to a human gate rather than spinning.
There is no `timeout(1)` on this host — use a Monitor / until-loop to wait on a
condition, never a blind foreground `sleep`.

---

## End-of-run teardown checklist

1. Board: reach a **clean menu prompt** → `G Y` → drain to EOF → `docker rm -f
   nextexpress-ref-<run>`.
2. Server: **kill by PID**.
3. Clear the run-state file (or mark complete).
4. Leave `nextexpress-bbs` uncorrupted: no orphaned phantom login, no lingering
   `acpConnections.dat` ban from this run.
