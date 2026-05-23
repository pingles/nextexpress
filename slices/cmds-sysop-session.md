# Tier G — Sysop session control

Local logon at the BBS console, node reservation and lifecycle, and
the sysop-kick action. These are the controls a sysop needs to keep
a running BBS healthy; they all touch the `Session` and `Node`
entities and therefore must coexist cleanly with the per-task
concurrency model.

See [SLICES.md](../SLICES.md) for the schema-growth principle and
the concurrency model section.

## Slice G1 — `F1` sysop direct logon

- **In Scope**
  - `session.allium:SysopDirectLogon` — F1-equivalent local key
    shortcut on the BBS console creates a session at
    `state = onboarded` for `sysop_user()` (`slot_number = 1`),
    `channel = sysop_console`, skipping identification / auth.
- **Out of Scope**
  - F2-style "local logon" (Slice G2).
  - `instantLogon` sysop key combo (`session.allium` open
    question).

## Slice G2 — Local logon + `RL` relogon

- **In Scope**
  - `LogonChannel::local` — F2 path: still goes through
    identification / auth, but `online_baud = 0` and
    `is_remote = false`.
  - Parser: `MenuCommand::Relogon` and the rule
    `session.allium:RelogonRequested` — session ends with
    `relogon`; `ReleaseNode` flips node back to `connecting`
    instead of `idle`.
  - Mirrors `internalCommandRL` (`amiexpress/express.e:25534`).
- **Out of Scope**
  - Sysop "switch user" UX wrapping relogon (slice G2b).

## Slice G2b — Sysop "switch user" UX

- **In Scope**
  - Sysop-console wrapper around the existing `RelogonRequested`
    rule: pick a target user from the account list, perform a
    relogon that lands as the chosen user without typing
    credentials.
  - Used during F6-style account maintenance to "log in as them and
    look around."
- **Out of Scope**
  - Audit trail beyond the standard `callersLog` entry. The act of
    becoming another user is logged like a regular relogon.

## Slice G3 — Node reservation

- **In Scope**
  - Adds `Node.reserved_for: Option<UserId>`, the `reserved` status
    and the `idle -> reserved -> idle` /
    `reserved -> connecting` transitions plus the
    `ReservedHasUser` invariant.
  - Rules `session.allium:ReserveNodeForUser` and
    `ClearNodeReservation`.
  - `AcceptConnection` rejects with `reserved_for_other` when the
    connecting user is not the reserved one.
- **Out of Scope**
  - The "page reserved-for-X user" out-of-band notification (slice
    G3b).

## Slice G3b — Page reserved-for-X user

- **In Scope**
  - When a node is reserved (`Node.reserved_for = Some(u)`),
    pinging the OLM channel of `u` if they're currently online
    surfaces a "your reservation is ready on node N" notification.
  - Reuses the OLM delivery channel from
    [cmds-comm.md](cmds-comm.md)'s E5.
- **Depends on**: E5 (OLM channel).
- **Out of Scope**
  - Notification to email / external — out-of-band channel is
    in-BBS only.

## Slice G4 — Node suspend / resume / shutdown

- **In Scope**
  - Adds the `suspended` and `shutting_down` statuses and the
    `idle -> suspended -> idle` and `idle -> shutting_down`
    transitions.
  - Rules `session.allium:SuspendNode`, `ResumeNode`,
    `InitiateShutdown`.
  - Cooperative shutdown — active sessions log off on their own
    clock per the rule's `@guidance`.
- **Out of Scope**
  - OS-level signal handling for graceful daemon stop (config
    concern).

## Slice G5 — Sysop kick

- **In Scope**
  - `session.allium:SysopKick` — sysop console command kicks a
    session on another node; `logoff_reason = sysop_kicked`.
- **Out of Scope**
  - Inter-node messaging (`OLM`); kick is a direct sysop action
    only.

## Slice G6 — Sysop in-session time adjust (`SV_TIMEINCREASE` / `SV_TIMEDECREASE`)

- **In Scope**
  - Adapter: sysop-console key combo (legacy F2 / F3 on a
    *connected* session) adjusts `loggedOnUser.timeTotal` by
    ±10 minutes on the active session running on another node.
  - Mirrors `amiexpress/express.e:7864-7876` (`SV_TIMEINCREASE`,
    `SV_TIMEDECREASE`).
  - Persists via `UserRepository`.
- **Out of Scope**
  - Adjusting time on the sysop's own console session — there's no
    `timeTotal` to deplete on `LogonChannel::local`.

## Slice G7 — Sysop display-file-to-user (`SV_DISPLAYFILE`) + capture (`SV_CAPTURE`)

- **In Scope**
  - Sysop picks a file on the host filesystem; the adapter streams
    it across the wire to the targeted session, byte-for-byte,
    legacy `startASend` at `amiexpress/express.e:7884-7889`.
  - Capture toggle: open a file at `<bbs-loc>/Capture/Node<n>.log`,
    append every byte the session sends and receives until toggled
    off (`amiexpress/express.e:7878-7882`).
- **Out of Scope**
  - Live-edit of the file mid-stream.

## Slice G8 — Sysop grant/revoke temporary access (`SV_GRANTTEMP`)

- **In Scope**
  - Sysop-key on the console swaps the targeted session's
    `secStatus`, `conferenceAccess` and `timeTotal` to a sysop-set
    "grant" tier; second invocation restores the originals.
  - Mirrors `amiexpress/express.e:7899-7921`.
  - This is the *in-session* version of the workflow the
    file-based conference-access model deliberately doesn't cover
    (see SLICES.md "Skipped slices" for the static-config side).
- **Out of Scope**
  - Persisting the grant past session end (legacy is in-session
    only).

## Slice G9 — Sysop availability toggle (`SV_CHATTOGGLE`)

- **In Scope**
  - Adds `Server.sysop_available: bool` (first written here).
  - Sysop-key on the console flips it; the `O` page-sysop branches
    in [cmds-comm.md](cmds-comm.md) read it (E1's "leave a
    comment" fallback fires when `false`).
  - Mirrors `amiexpress/express.e:7923-7930`.
- **Out of Scope**
  - Auto-availability based on idle time.

## Slice G-wire — Tier G wire-and-smoke

- **In Scope**
  - Spawn binary, F1-logon, reserve node 2 for `alice`, attempt
    connect as `bob` (rejected), connect as `alice` (admitted),
    suspend node 2 from sysop console, attempt connect as `alice`
    (rejected), resume, shut down node 2 (active session logs off
    cooperatively).
- **Out of Scope**
  - The chat protocol from
    [cmds-comm.md](cmds-comm.md)'s Slice E2 — it depends on G1 /
    G5 but lives in its own tier.
