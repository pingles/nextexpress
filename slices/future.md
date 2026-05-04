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
