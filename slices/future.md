# Future phases (not yet sliced)

The following are listed for completeness so the slice plan can grow into
them without reshaping earlier slices. They will be broken down when
their phase becomes the next focus.

See [SLICES.md](../SLICES.md) for the schema-growth principle, progress
table and asset inventory.

- **SSH transport adapter** — second wire protocol after Telnet (Slice 8). Plugs into the same per-task accept loop; no new domain rules. Likely uses `russh` or similar. Defining a `Transport` trait that both Telnet and SSH implement falls naturally out of this slice.
- **FTP control adapter** (`session.allium:LogonChannel.ftp` + `amiexpress/ftpd.e`).
- **HTTPd** (`amiexpress/httpd.e`) for webby file listings.
- **QWK packet generation** (excluded from current specs — see `messaging.allium` Scope).
- **FTN gateways** (`amiexpress/ftn.e`).
- **External / custom message bases** (`core.allium` open question).
- **OLM (online messages between nodes)** — referenced by `Session.quiet_mode` but not yet specified.
- **Multi-language translator** (`core.allium` open question).
- **IEMSI auto-handshake** (excluded from `session.allium` scope).
- **axSetupTool replacement** — config-file editor; per `AGENTS.md`, GUI is replaced by file editing, but a CLI wizard for first-run is plausible.
- **Xmodem / Ymodem / Hydra transfer protocols** — only Zmodem is sliced today (`cmds-files-transfer.md`'s D-T1). The other three exist in legacy AmiExpress (`amiexpress/xpr*.e`) and would land as drop-in alternative adapters behind the same `Transfer` entity, picked per `User.preferred_protocol`.
- **OS-level signal handling for graceful daemon stop** — sliced out of `cmds-sysop-session.md`'s G4 (deferred as "config concern"). Lands when the supervisor / systemd integration story is written; not a menu command, so it doesn't belong in any A–J tier.
- **Browse-side smoke against a real lha** — `cmds-files-list.md`'s D-wire uses fixtures. A future smoke once an lha extractor is wired into the test harness would let the smoke exercise the real on-disk layout.
- **Sysop bulk file import** — sliced out of `cmds-files-sysop.md`'s D-S1. The natural home is the CLI wizard listed above; until that wizard exists, sysops drop files in the area directory by hand and rerun the indexer.
