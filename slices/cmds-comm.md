# Tier E — Communication

`O` (page sysop), `OLM` (online message between nodes), and the
`WHO` / `WHD` who's-online listings. These slices land after the
quick-wins in Tier A because two of them depend on
`Session.quiet_mode` from slice A9 and one of them depends on a
working notion of "other nodes" that grows here.

See [SLICES.md](../SLICES.md) for the schema-growth principle and
asset inventory.

## Slice E1 — `O` (page sysop), comment-only branch

- **In Scope**
  - Adds `User.chat_minutes_remaining`,
    `User.chat_minutes_per_call` (first read here).
  - Parser: `MenuCommand::PageSysop`.
  - When the sysop console is unavailable (no chat protocol yet),
    falls back to the `commentToSYSOP` path
    (`amiexpress/express.e:25372-25404`), reusing the existing
    `PostCommentToSysop` rule. Wire text mirrors
    `Sorry, <sysop>, is not around right now\n\nYou can use 'C' to leave a comment.`
- **Out of Scope**
  - Two-way live chat protocol (Slice E2).
- **Why this branch lands first**: it delivers visible user value
  (the legacy fallback) without needing the chat protocol
  subsystem.

## Slice E2 — `O` (page sysop), live chat path

- **In Scope**
  - Decrement `chat_minutes_remaining`; reject when zero.
  - Pages the sysop console (out-of-band notification to whatever
    `cmds-sysop-session.md`'s F1 logon path provides).
  - Two-way chat protocol over the existing telnet stream.
  - Sysop-side accept / decline UI on the console: incoming page
    raises a notification, sysop presses a console key combo to
    accept (drops both sessions into the chat protocol) or decline
    (falls back to E1's comment branch on the user side).
  - **Wire-and-smoke** (folded in, not deferred to E-wire): spawn
    two clients; one types `O`; assert the sysop console sees the
    page notification, accepts, both sides see the chat banner,
    bytes flow both ways, either side can type `/Q` to exit.
- **Out of Scope**
  - Multi-sysop page routing — single sysop console only.

## Slice E3 — `WHO` (who's online, summary)

- **In Scope**
  - Parser: `MenuCommand::WhoOnline`.
  - Iterates the node-pool (`rust/src/app/node_pool.rs`) and emits
    the legacy `who(0)` listing (`amiexpress/express.e:26097`):
    one row per active node with user name + action.
- **Out of Scope**
  - Multi-node clustering — single-process for now.

## Slice E4 — `WHD` (who's online, detailed)

- **In Scope**
  - Same as E3 but emits the legacy `who(1)` detailed form (action,
    location, conference, baud, time-on).

## Slice E5 — `OLM` (online message between nodes)

- **In Scope**
  - Parser: `MenuCommand::OnlineMessage(args)` — supports the
    interactive form (no args, prompts for destination node and
    drops into the line editor) and the one-liner form
    (`OLM <node> <message>`) per `internalCommandOLM`
    (`amiexpress/express.e:25406-25502`).
  - Honours `Session.quiet_mode` on the recipient — quiet sessions
    silently drop the message; the sender sees the legacy
    "NODE N HAS MESSAGES SUPPRESSED" banner.
  - Adds a `Notifications` channel to the node-pool so messages
    cross task boundaries.
- **Out of Scope**
  - File attachments to OLMs.

## Slice E-wire — Tier E wire-and-smoke

- **In Scope**
  - Spawn the binary with two simulated clients; run `O` (comment
    branch), `WHO`, `WHD`, `OLM` between the two; assert legacy wire
    bytes and that quiet-mode actually suppresses delivery.
- **Out of Scope**
  - Live-chat smoke — folded into E2 itself rather than deferred.
