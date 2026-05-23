# External commands (`WALL`, doors, BBS commands)

The legacy menu has labels (`WALL`, plus per-sysop additions like
`ACCOUNTS`, `EDITOR`, `FAX`, `FULLEDIT`, …) that are **not** entries
in `processInternalCommand` (`amiexpress/express.e:28285`). They are
served by `runSysCommand` / `runBbsCommand`
(`amiexpress/express.e:28258-28282`) — the legacy "external command"
mechanism that walks `BBS:Commands/SysCmd/` and
`BBS:Commands/BBSCmd/` looking for a matching `.info` tooltype and
runs it as a door / shell-out.

Per `AGENTS.md`, NextExpress stores config in files rather than icon
tooltypes. The slices below add the equivalent file-based dispatcher
and the headline external command (`WALL`) so the default
`Conf02/Menu.txt` doesn't have a typeable label that the BBS
silently rejects.

See [SLICES.md](../SLICES.md). The deeper door / shell-out subsystem
that this leans on is also referenced from
[cmds-sysop-console.md](cmds-sysop-console.md)'s `0` slice (remote
shell uses the same `spawn-and-proxy` adapter).

## Slice X1 — External command dispatcher

- **In Scope**
  - New module `app::external_command`: walks a
    `[external_commands]` block in `nextexpress.toml`; each entry
    has a token (`WALL`, `EDITOR`, …), an executable path, the
    set of arguments to pass and an access-level gate.
  - `MenuFlow::run` dispatches an unknown command token through
    this module before falling back to the
    `Unknown command. Type G to log off.` notice.
  - Bytes from the spawned process are proxied to the user; stdin
    is piped from the session.
- **Out of Scope**
  - Per-conference external command overrides (legacy supports
    them — defer).
  - Inter-node IPC for the spawned process — single-node only.

## Slice X2 — `WALL` (write-on-the-wall)

- **In Scope**
  - Ships a built-in implementation of `WALL` as a NextExpress
    external command: prompts for a one-line message, appends it
    to `<bbs-loc>/WallOfMessages.txt` with the user's name and a
    timestamp, then optionally displays the last N entries.
  - The legacy `WALL` was supplied per sysop; we ship a default so
    the Conf02 menu label is honoured out of the box.
- **Out of Scope**
  - Threading / replies — single flat list.

## Slice X3 — Per-conference external command overrides

- **In Scope**
  - Reads an optional `[external_commands]` block in
    `Conf<n>/conference.toml`; entries here are overlaid on the
    global table from slice X1 for sessions joined to conference N.
  - Mirrors the legacy "BBS:Commands/BBSCmd/" per-conference
    override path that `runBbsCommand` walks
    (`amiexpress/express.e:28258-28282`).
- **Out of Scope**
  - Negative overrides (a per-conference *removal* of a global
    command); the spec doesn't model it.

## Slice X-wire — External commands wire-and-smoke

- **In Scope**
  - Smoke test: type `WALL`, see the prompt, leave a message, type
    `WALL` again, see the last entry. Type an unknown token (e.g.
    `NOTACOMMAND`), confirm the legacy unknown-command notice
    still fires.
- **Out of Scope**
  - Concurrent writes to `WallOfMessages.txt` — the slice uses a
    per-area lock as elsewhere; stress-testing the lock is its own
    concern.
