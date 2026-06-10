# D1+D2 — `F` file listings: NextScan (AquaScan-parity) over a seeded in-memory FileRepository

**One visible increment.** `cargo run nextexpress.toml`, telnet 2323, sign in as the seeded sysop, type `F` → NextScan banner + `Directories: (1-2), …` prompt; `F 1` → the framed 27-file listing, page-by-page, `[ End of File List ]`, `Q` → `Quit` → menu. Never empty out of the box.

**Parity target (final, per user decision):** the AquaScan v1.0 door experience with NextScan branding. Ground truth is the live captures — `comparison/transcripts/ae_tierd_aquascan3.txt` (S1–S10, cleanest) and `ae_tierd_aquascan.txt` (A1–A8, E1–E3). Every byte below was re-verified against the transcripts during this synthesis pass. The stock `internalCommandF` path (`amiexpress/express.e:24877`, `displayFileList :27626`, `displayIt :27719`) is recorded only as the documented stock diff in COMMAND_PARITY.md.

---

## 1. The wire contract (byte-exact, all re-verified)

Door-derived strings carry capture citations (`// comparison/transcripts/ae_tierd_aquascan3.txt:NNN`) instead of express.e lines; the three NextScan rebrands additionally carry deliberate-departure notes (SLICES.md departure convention). **All frame/banner/art constants are `&[u8]` byte-string literals** — the art and © are single Latin-1 bytes (`\xb8 \xf8 \xa4 \xb0 \xac \xaf \xa9`); `&str`/`\u{}` constants would emit multi-byte UTF-8 and break parity (departure from the wire_text.rs `\u{00A9}` habit, recorded in the slice doc). Width tests count visible columns (bytes excluding `ESC[..m` sequences).

### 1.1 Entry preamble (every arg form: `F 1`, `F A`, `F U`, `F H`, `F 99`, bare `F`)
After the menu loop's command echo (`F 1\r\n`):
```
\x1b[0m\r\n
\x1b[0m\x1b[34m--[ \x1b[36mNextScan v1.0 by NextExpress/Rust \x1b[34m]---------------[ \x1b[36m'f ?' for options \x1b[34m]--\x1b[0m\r\n
\r\n
```
(aquascan3.txt:163/217/257. Centre label 34 incl. trailing space; 15 dashes; right label `'f ?' for options ` = 18; visible total 77 — see §7 width math.)

### 1.2 Scan headers (plain, no ANSI)
- `Scanning dir N from top... Ok!\r\n` + `\r\n` before the first frame line (aquascan3.txt:217).
- `Scanning dir N from top... Nothing found!\r\n` — **no footer, no More?, no blank line after**; straight to the exit tail (ae_tierd_aquascan.txt:515-527, E2).
- `Scanning HOLD dir from top... Nothing found!\r\n` for `F H` with no held files (aquascan3.txt:675-687, S9).
- Out of range (incl. provisional `F 0`): banner preamble + `The highest directory number is {max}!\r\n` + exit tail (aquascan.txt:330-342, A7; number flexes).

### 1.3 Listing frames — **date-group separators, per-dir footers** (the rule three proposals got wrong)
- **Separator block** is emitted before a framed file only when it is the **first framed file of the dir** or its MM-DD-YY **differs from the previous framed file's** date. Same-date framed neighbours follow the previous row/continuation **directly** with their header — no blank, no separator (MYDEMO 06-10-26 → TOOLPACK 06-10-26, aquascan3.txt:152-156). Plain rows are invisible to grouping (DISKMAST #16 → MEGADEMO plain (different date, still no sep) → sep → XPRZMODM #17, aquascan3.txt S7 repr :490). Block bytes:
```
\x1b[0m\r\n
\x1b[0m<44sp>_\xb8,\xf8*\xa4\xb0\xac\xb0\xa4*\xf8,\xb8_\xb8,\xf8*\xa4\xb0\xac\xac\xb0\xa4*\xf8,\xb8_\r\n
\x1b[0m<6sp>\xb8,\xf8*\xa4\xb0\xac\xaf\xac\xb0\xa4*\xf8,\xb8_\xb8,\xf8*\xa4\xb0\xac\xb0\xa4*\xf8, MM-DD-YY\r\n
\x1b[0m\r\n
```
- **Header**: `\x1b[0m\x1b[34m[\x1b[0m File #N \x1b[34m]` + pad + `- ---- ` + 34 dashes + ` ---- -\r\n`. Pad = 31 − visible_len(`[ File #N ]`) → 20 spaces for 1-digit N, 19 for 2-digit (aquascan3.txt:146 vs S7 repr `File #10`); dash run always starts at visible col 31, total 79.
- **Framed row**: `\x1b[0m\x1b[36m` + name padded (never truncated) to 13 + `\x1b[34m` + check byte + `\x1b[32m` + size right-justified 7 + `  ` + `\x1b[33m` + MM-DD-YY + `\x1b[0m  ` + first description line + `\r\n` (aquascan3.txt:147). Continuations: `\x1b[0m\x1b[0m` + 33 spaces + text + `\r\n` (:154).
- **Plain fallback**: a file is *frameable* iff `name.len() < 13 && size.count() <= 9_999_999`. Unframeable rows emit `\x1b[0m\x1b[0m` + the raw legacy DIR row + `\r\n`, consume **no** File# and trigger **no** separator (MEGADEMO.DMS 8-digit drift, THIRTEENCH.LZ and README1ST.TXT 13-char names, ALONGFILENAME.LHA — aquascan3.txt S7 repr :490).
- **Footer**, directly after the last row (no preceding blank): `\x1b[0;34m[\x1b[36m End of File List \x1b[34m]\x1b[0m\r\n` (:157). **Per dir**: in `F A` each non-empty dir gets its own footer + post-End More?; File# **resets to #1** per dir (aquascan3.txt S8 repr :673). `Nothing found!` dirs get neither (provisional for an empty *middle* dir in `F A`: emit its scan line, blank, then the next dir's scan line — UNVERIFIED).

### 1.4 Legacy DIR row (the pure layer-1 renderer, used verbatim for plain rows)
From the upload writer (`amiexpress/express.e:19450-19509`, size at `:18918-18942`, offsets cross-checked by `ftpd.e:1093-1132`), reproduced runtime from `File` fields: name left-justified min-width 13 **without truncation** (Amiga `\l\s[13]` pads, never cuts — do NOT reuse wire_text's truncating `left_field`, rust/src/app/wire_text.rs:825); check byte poked at byte 13 only when `name.len() < 13` (space when `None`); size `\r\d[7]` RJ-7 for ≤ 9,999,999 else unpadded (authentic column drift); 2 spaces; `MM-DD-YY` (time crate `format_description!("[month]-[day]-[year repr:last_two]")`, UTC); 2 spaces; first description line. Continuations = exactly 33 spaces + text. Verified against all 6 fixture edge rows (`comparison/evidence-tierD/fixtures/Dir1`). CREDITBYKB/CONVERT_TO_MB variants: deferred (untoggled on the reference board).

### 1.5 Pager — `ScanPager`, private to file_list (shared `menu_flow::pager` untouched; its `(Pause)...More(y/n/ns)? ` bytes stay pinned for L/MS)
**Prompt** (aquascan3.txt:158):
```
\x1b[0;36mMore? \x1b[32m(\x1b[33mY\x1b[32m/\x1b[33mn\x1b[32m/\x1b[33mns\x1b[32m)\x1b[36m, \x1b[32m(\x1b[33mC\x1b[32m)\x1b[36mlear, \x1b[32m(\x1b[33mF\x1b[32m/\x1b[33mR\x1b[32m)\x1b[36m Flag, \x1b[32m(\x1b[33m?\x1b[32m)\x1b[36m Help, \x1b[32m(\x1b[33mQ\x1b[32m)\x1b[36muit:\x1b[0m 
```
Prompt clears are overprints, never `ESC[K`: `\r` + **69 spaces** + `\r` after More?/ns-confirm; `\r` + **79 spaces** + `\r` after a flag entry (both counted programmatically from the transcripts this pass).

**Input mechanism — decision.** `TerminalEcho` has only `Visible | Masked` (rust/src/app/terminal.rs:19-24) and `read_telnet_line` echoes per-char, `\r\n` on Enter, and `BS SP BS` in **both** modes (rust/src/adapters/telnet_line.rs:100-127) — so no existing read can be byte-silent, and the proposals' `TerminalEcho::Hidden` does not exist. This unit adds a third variant **`TerminalEcho::Silent`** (no per-char echo, no Enter `\r\n`, no BS echo) threaded through `EchoMode` (telnet_line.rs:30), the `From` impl (telnet_listener.rs:196-201), and every test fake — explicit, separately-tested adapter work. All pager sub-prompts use Silent reads and the **handler emits every captured echo byte itself**; the Directories prompt stays `Visible` (the door's answer echo `2\r\n` is captured, aquascan3.txt:163). `Terminal::read_key` (true hotkeys) is deferred to named slice **D2b**; until then the user presses Enter after each pager key — a COSMETIC/ergonomics divergence in COMMAND_PARITY.md (server-emitted bytes identical).

**Verb table** (Silent line read; first chars decide):
- `Y` / empty / unknown → clear (`\r`+69sp+`\r`), continue, counter := 0. (Unknowns-continue is UNVERIFIED.)
- `n` mid-list → echo `n`, clear, stop with the listing exit tail (UNVERIFIED — only post-End `n` is captured).
- `n` post-End → echo `n`, hold pending; the **next** line first emits `\x08 \x08` (erasing the n), then its verb runs (`n` … `\x08 \x08Quit`, aquascan3.txt:158-163, S2).
- `ns` → echo `n`, clear, `\x1b[36mNon-stop scrolling! Are you sure \x1b[32m(\x1b[33mY\x1b[32m/\x1b[33mn\x1b[32m)\x1b[36m? ` (Silent read); `Y` (unechoed) → clear, stream remainder non-stop (aquascan3.txt:361 + S7 repr :490); other answer → clear, resume paged (UNVERIFIED).
- `C` → `\r` + `\x0c` (form feed), **no echo, no clear, no re-prompt** — listing resumes immediately, counter := 0 (aquascan3.txt:292-321, S6). Ships now (needs no flag state).
- `F` → clear, `\x1b[36mFile name(s) to flag:\x1b[0m ` Silent read; handler echoes the input verbatim (**no trailing CRLF** — aquascan3.txt:212-217, S4); `\r`+79sp+`\r`; redraw More?. Input **read and discarded** — flagging is silent in the captures, so this is wire-identical until D5 wires FlaggedFile.
- `R` → same but `\x1b[36mFile number(s) to flag:\x1b[0m ` (aquascan3.txt:252-257, S5 — distinct prompt, separate test).
- `?` → clear, full help screen (§1.7), redraw More? (UNVERIFIED — A2 was polluted; recapture wishlist).
- `Q` → `Quit\r\n` (no clear first — aquascan3.txt:321) + exit tail.

**Post-End More?** is unconditional after each dir's footer in paged mode, even for 3-file listings (aquascan3.txt:157-158); `Y` → clear, then if more dirs: `\r\n` + next `Scanning dir N from top...` (S8 repr :673); if last dir: exit tail. Suppressed entirely in non-stop mode (footer → tail directly, S7 repr :490).

**Page accounting — decision.** Counter counts every `\r\n`-terminated line the handler emits (preamble included), starting at 0; More? fires at **29**; counter resets to 0 on resume, on `C`, and (chosen, uncaptured) at each dir transition. This reproduces the captured pages 1 and 2 exactly (29/29) and drifts by one line from page 3 (captured 28/28/28 — fitted against every S8/S6/S4 boundary this pass; no simple counter reproduces the drift, which looks like a door quirk). The threshold is a **NextScan constant**, not `user.line_length()` — AquaScan owns its paging via its own `F W` config (evidence: the stock pager paused at ~31 lines for the same account). Mid-list More? *positions* from page 3 onward are a documented COSMETIC divergence; all surrounding bytes are identical. More? may legitimately split a frame (fires between header #13 and its row, and between sepA/sepB — aquascan3.txt:582, :673) — hence: **materialise the dir's rendered lines first** (FILES.md:370-373 "read, materialise, release"; no repo call after a pause), then stream line-by-line through the pager.

### 1.6 Bare `F`, aborts, errors (per-path tails — no shared exit-tail helper)
- Bare `F`: preamble + `\x1b[36mDirectories: \x1b[32m(\x1b[33m1-{max}\x1b[32m)\x1b[36m, \x1b[32m(\x1b[33mA\x1b[32m)\x1b[36mll, \x1b[32m(\x1b[33mU\x1b[32m)\x1b[36mpload, \x1b[32m(\x1b[33mH\x1b[32m)\x1b[36mold, \x1b[32m(\x1b[33mEnter\x1b[32m)\x1b[36m=None ?\x1b[0m ` (reset before trailing space; `1-2`/`1-1` flexes — aquascan3.txt:163, aquascan.txt E3 :529-539). **Visible** line read. `H` offered unconditionally (only a sysop was captured; gating deferred, noted).
- Answers: number/A/U/H → same spans as args (A/U/H at the prompt are UNVERIFIED-reasonable); after the echo, `\r\n` then the scan header (aquascan3.txt:163).
- Enter → echo `\r\n` + `\r\n` + **one** `\x1b[0m\r\n` + menu (aquascan3.txt:165-177, S3).
- Junk answer → echo + `\r\n` + `Error in input!\r\n` + `\r\n` + **one** `\x1b[0m\r\n` + menu (aquascan.txt:109-120, A2 — note: A2 shows no banner because the prompt was already pending; the error shape itself is the captured fact).
- Listing exits / `Q` / `F 99` / `Nothing found!` spans: **two** `\x1b[0m\r\n` before the menu prompt (aquascan3.txt:163, :177-vs-:163 contrast, aquascan.txt:342, :527).
- Menu-level junk (`F XYZ`), leading-`R` (`F R 1`), `Q` token, `F W`: banner preamble + `\r\nError in input!\r\n\r\n` + one `\x1b[0m\r\n` (UNVERIFIED shape for the banner question; each is a tagged entry — `F R` is a *temporary* divergence the D3 slice flips, `Q` token waits for a quick-scan capture, `F W` is a *permanent* departure: NextExpress config is TOML, AGENTS.md).
- `F 0`: dispatch range check `1..=max` → highest-dir error (UNVERIFIED).
- Zero-area conference (never hit by seed): highest-dir error with max 0 from both bare `F` and args (UNVERIFIED stopgap).

### 1.7 `F ?` help screen (aquascan3.txt:100-129, S1)
`F ?\r\n` echo + `\x1b[0m\x0c\r\n` + help banner (`--[ ` + centre 34 + `]` + **9 dashes** + `[ ` + right 26 + `]--`, visible 79) + `\r\n\r\n` + the captured body verbatim (five `\x1b[0m  F ...` rows with `\x1b[36m-` descriptions at visible col 31, then the plain tree diagram) with `  F W … \x1b[36m- Configure NextScan` + epilogue `\x1b[0m\r\n\x1b[0m\x1b[0m\r\n\x1b[0m\r\n` + menu prompt.

---

## 2. Domain model (`rust/src/domain/files/`) — schema-growth-trimmed

New `pub mod files;` in domain/mod.rs. Pure: no `std::fs`/tokio/serde (rust/tests/architecture.rs:14-22, :106). Timestamps are `SystemTime` (house convention, domain/conference_visit.rs:44).

```rust
/// spec: core.allium:316-324. Browse reads conference + number only;
/// `name`/`upload_path`/`free_downloads` deferred (first reader: Z header / upload / BeginDownload files.allium:251).
pub struct FileArea { conference: u32, number: u32 }   // ctor + accessors

/// spec: files.allium:49-56 — ONLY the variants browse reads.
pub enum FileStatus { Available, Lcfiles, HeldForReview }
impl FileStatus {
    /// Listing-visible set {available, lcfiles} — files.allium:52-53, FlagFile :165,
    /// invariant FlaggedFilesAreDownloadable :492-495. Lcfiles is kept because the
    /// spec pins the visible SET; encoding {available} alone would be spec-wrong.
    pub fn is_listing_visible(self) -> bool;
}
// Deferred variants + the transition table: in_playpen/quarantined (upload/check writers,
// files.allium:104-119), removed (maintenance). Encoding them now is dead enum surface.

pub struct File {
    conference: u32, area: u32,
    name: String,                 // 12-char convention; >=13 renders plain (capture-attested)
    size: Bytes,
    status: FileStatus,
    check_char: Option<u8>,       // raw byte poked at DIR col 13 (P/F/N/D/checksym,
                                  // express.e:19458-19470) — upload-writer DATA, not
                                  // derivable from FileStatus (BADUPLD is check-'F' yet
                                  // Available+framed). u8 so a high-bit checksym can never
                                  // UTF-8-split. None = authored row had no char.
    description: String,          // whole listing text incl. continuation lines ('\n'-separated;
                                  // files.allium:94). 'Sent by:' is just continuation text here.
    uploaded_at: SystemTime,
}
impl File { pub fn description_lines(&self) -> impl Iterator<Item = &str>; }
```
Deferred File fields, first reader named: `uploaded_by` (upload slices / SENTBY_FILES), `description_source` + privacy (UploadDescriptionEntered files.allium:328-338), `file_id_diz*` (DIZ slice), `last_downloaded_at`/`download_count` (CompleteDownload :274-275).

**Bytes** (rust/src/domain/bytes.rs:13-28 — today new()/count(), Ord already derived):
```rust
#[must_use] pub const fn saturating_add(self, rhs: Bytes) -> Bytes // tallies, files.allium:270-281, :355-361
#[must_use] pub const fn saturating_sub(self, rhs: Bytes) -> Bytes // zero-floor, files.allium:227-232; invariants :497-505
```
No browse rule does Bytes arithmetic — this is D1's one piece of named groundwork, with direct boundary tests so cargo-mutants has killers. Saturating (not `impl Add`): the spec pins non-negativity; no panic paths.

## 3. FileRepository port (`domain/files/repository.rs`)

Sync, domain-side, UserRepository style (rust/src/domain/user_repository.rs:65-105); **unbounded trait, bounds at the alias** (house style — `Send + Sync` lives on `SharedFileRepo`). Rule-named and narrow (designs/FILES.md:389-392); infallible `Vec` returns — the only adapter this unit ships cannot fail, and Result plumbing nothing produces is untestable mutant surface (the D2s SQLite slice grows the error type, compiler-driven across exactly two call sites).

```rust
pub trait FileRepository {
    /// Areas of `conference`, ascending by number. Feeds the (1-N) prompt bound,
    /// the highest-dir check, the A span, and U = highest-numbered area
    /// (legacy fLLoop = maxDirs, express.e:27662; capture: F U -> "Scanning dir 2",
    /// aquascan.txt:344-371 — concretises upload_area_for's default, files.allium:543-546).
    fn areas_in_conference(&self, conference: u32) -> Vec<FileArea>;

    /// Listing-visible files (FileStatus::is_listing_visible) of one area,
    /// uploaded_at ascending, ties broken by insertion order (the Dir2
    /// MYDEMO-before-TOOLPACK same-date pair is byte-observable). FR (D3)
    /// is a reversal over this same materialised Vec — near-free.
    fn find_in_area(&self, conference: u32, area: u32) -> Vec<File>;

    /// held_for_review files — the (H)old span (legacy <confDir>hold/held,
    /// express.e:27687-27688). FILES.md's list_by_status narrowed to the one
    /// status browse reads.
    fn list_held(&self, conference: u32) -> Vec<File>;
}
```
Deferred methods, first reader named: `list_new_since` (D9/N), `search_descriptions` (D4/Z), `find_metadata` (transfer slices). Handlers call the port inline from the async menu loop (house precedent: user repo).

## 4. Storage-vs-rendering assessment (steer A) — verdict: **runtime generation**

- **(a) DIR-file artefact streamed from disk** — the *stock* mechanism (the file IS the wire, displayIt express.e:27719). But the parity target is AquaScan, which **itself parses the DIR file and re-renders every row** with colour/frames at runtime. Under the chosen target (a) still renders at runtime, while adding: a write-time formatter for every future mutation, a parser over our own artefact (two format-knowledge sites), a second source of truth beside SQLite (contradicting FILES.md:53-62), `std::fs` in the serving path, and a worse no-disk dev boot. Steer B removed its only remaining rationale (existing legacy files). **Rejected.**
- **(b) Runtime generation from repository data** — one pure `dir_row(&File)` owns the upload-writer's format rules; the AquaScan framer renders over `File` fields, with frameability decided structurally (`name < 13 && size <= 9_999_999` — same outcomes as the door's parser for every capture-attested shape, including the authentic drift/fallback rows, with less code). Performance is a non-issue: ~50K rows worst case board-wide, ~26K typical (designs/FILES.md:22-49); one dir materialises in microseconds. All parity risk concentrates in two pure functions byte-pinned against the checked-in fixture corpus + live captures. **Chosen.**
- **(c) Hybrid/cached rendering** — banned speculation (FILES.md:422-434). **Rejected.**

**What supersedes the dropped D1 `Conf<n>/Dir<m>` loader (steer B):** the seeded `InMemoryFileRepository` (dev default, this unit) + `SqliteFileRepository` (production, slice D2s). No legacy DIR reader or writer will ever exist; `legacy_dir.rs` is gone from the FILES.md adapter layout and the round-trip nail-down (FILES.md:394-398) resolves "neither" (verify the doc — partial edits already landed). No deferred import slice is scheduled (omitted; if ever wanted it follows USERS.md:322-325's separate-ingest-tool posture, never a runtime adapter).

## 5. Adapters + SQLite timing + config story

**This unit ships one adapter:** `rust/src/adapters/in_memory_file_repository.rs` — plain owned `Vec<FileArea>` + `Vec<File>` fields (read-only port → **no Mutex**; the user repo's Mutex exists because its port writes), filtering via the domain predicate so adapter and spec can't drift. Mirrors in_memory_user_repository.rs's role as the zero-config production default (rust/src/bootstrap.rs:137-169).

**SQLite — DECISION: deferred to named slice "D2s — files SQLite metadata store"**, inserted in slices/cmds-files-list.md between D2 and D3, scheduled **no later than the first file-writer slice** (sysop upload / upload) and before any deployment needing real data. Justification: (1) FILES.md mandates SQLite as production truth (:53-62) but is silent on timing; the realised repo precedent is in-memory first, SQLite later behind a pre-reserved key (in_memory_user_repository.rs:3-5; `user_storage`). (2) **Nothing can write real rows yet** — no upload path, no sysop import, no legacy ingest (steer B): a SQLite adapter today could only serve the same dummy seed, pure schema-growth violation. (3) The unit is already the largest Tier-D slice (new port, renderer, pager, Silent echo mode, smoke). D2s contract, written into the slice doc now: copy sqlite_user_repository.rs wholesale (`Mutex<Connection>`, WAL/foreign_keys/synchronous=NORMAL init_schema :96-100, `in_memory()` ctor) + `busy_timeout` (fixing the noted omission); `files` table trimmed to the browse columns + `idx_files_area_uploaded_at` (FILES.md:216-233) with a rowid insertion tiebreak; **demo records are never seeded into SQLite** (a file-less board is usable; `None`-key selection is the trigger, not emptiness — empty store lists `Nothing found!`); two-boot persistence smoke mirroring tests/sqlite_user_storage_smoke.rs; FileArea definitions leaning `[[file_area]]` in conference.toml (config-via-files house rule; needs a serde-defaulted field under `deny_unknown_fields`, file_conference_repository.rs:156) — settled in D2s.

**Config story for sysops:** none this unit (a key nothing reads violates schema-growth; setting it early would be an honest unknown-field parse error). D2s adds `file_storage: Option<PathBuf>` mirroring `user_storage` exactly (rust/src/app/config.rs:138-146): `None` → seeded in-memory (+ stderr notice), `Some(path)` → SQLite created on first run. The key name is reserved in the slice doc now.

## 6. Seed story

- `app/seed.rs` gains `demo_file_catalogue(conferences: &[Conference]) -> (Vec<FileArea>, Vec<File>)` beside `default_sysop` (:44), invoked only by bootstrap (sole composition root, bootstrap.rs:1-19) with an eprintln notice mirroring :144-167 ("file listings are in-memory demo records; persistent storage lands with `file_storage`, slice D2s").
- **Contents — the full Tier-D fixture corpus, aligned to the landing conference** (resolves the P2/P3 seed/smoke contradiction): the **first** loaded conference (Conf01 "Main" — where the seeded sysop's auto-rejoin lands, domain/conference_visit.rs:278-295) gets areas 1+2 = `comparison/evidence-tierD/fixtures/Dir1` (**27 entries** — verified; includes README1ST.TXT) + `Dir2` (3 entries, with the same-date MYDEMO/TOOLPACK pair that discriminates the date-group rule). Every other conference gets area 1, empty — mirroring the reference "New Users" shapes (`(1-1)` prompt, `Nothing found!`, aquascan.txt E2/E3).
- Records mirror SysopUploadFile's output shape (files.allium:431-449): status Available; `check_char` = `Some(b'P')` except BADUPLD.LHA `Some(b'F')` and `None` for the three no-char rows (THIRTEENCH.LZ, ALONGFILENAME.LHA, README1ST.TXT — never rendered at col 13 anyway); fixture dates as fixed mid-day-UTC `SystemTime` constants (deterministic MM-DD-YY on any TZ); `Sent by: SYSOP` as PTREPLAY's continuation text; descriptions within the 44-char legacy bound (files.allium:558-560). No held, no lcfiles records (wire stays capture-identical; those variants are exercised by unit tests only).
- Because the corpus and order match the live board, the `F 1` first page (29 lines), File# numbering incl. 2-digit pads, plain-row fallbacks, and the `F U`/`F 2` trio are **directly byte-comparable to the captures modulo the three branding swaps** — pager positions from page 3 excepted (§1.5).

## 7. NextScan branding — three byte-length-identical swaps, **zero dash flexing**

| # | AquaScan original (verified) | NextScan replacement | width |
|---|---|---|---|
| 1 | listing+help banner centre `AquaScan v1.0 by Aquarius/Outlaws ` (34) | `NextScan v1.0 by NextExpress/Rust ` | 34 = 34 |
| 2 | help banner right `Copyright \xa9 1994 Aquarius ` (26) | `Copyright \xa9 2026 NextScan ` | 26 = 26 |
| 3 | help line `- Configure AquaScan` (20) | `- Configure NextScan` | 20 = 20 |

Dash runs unchanged (15 listing / 9 help; D3's `'fr ?'` variant uses 14+19, D9's `'n ?'` 15+18 — all verified). Listing banner visible width 77, help banner 79. Unit tests assert visible-width equality with the captured originals. The `\xa9` stays a Latin-1 byte inside `&[u8]` consts.

## 8. Parser, dispatch, composition

**Parser** (rust/src/app/menu_command.rs — supersedes D2's `FileList(NumberArg)` wording):
```rust
/// F — file listings via the NextScan lister (AquaScan door parity; shadowed
/// internal: internalCommandF, amiexpress/express.e:24877).
FileList(FileListArg)
pub(crate) struct FileListArg { pub span: FileSpan, pub non_stop: bool }
pub(crate) enum FileSpan { Prompt, Help, All, Upload, Hold, Dir(u32), Invalid }
```
`parse_file_list_command` follows `parse_join_command` (:282): head `F` case-insensitive; `?` → Help; `A`/`U`/`H` case-insensitive; numeric via `val_prefix` (:173) → `Dir(n)` (range check in the handler); trailing `NS` sets `non_stop` (**ships** — suppresses More? incl. post-End, matching the captured non-stop tail; silently ignoring it would paged-list a non-stop request); leading `R`, `Q` token, `W`, junk → `Invalid` → Error-in-input path with pinned tests (`F R 1` test carries a "temporary divergence until D3 flips this to reverse" comment). `FR`/`N` stay Unknown (:708-714; D3/D9). Compile gates: `advertised_token` (:1015) returns `"F"`; `every_menu_command` (:1045) gains a sample; Conf02/Menu5.txt gains the `F` row (test :972; stale RP/FW/K/MV/EH rows left for their own hygiene change).

**Module layout** (per SYSTEM.md refactoring 9 — single-consumer strings live with the command, not wire_text.rs): `rust/src/app/menu_flow/file_list/` = `mod.rs` (handler + span resolution), `dir_row.rs` (layer-1 renderer), `wire.rs` (`&[u8]` consts + render fns + byte tests), `pager.rs` (`ScanPager`). Registered via `mod file_list;` (menu_flow/mod.rs:15-24) + one dispatch arm (:162). Handler resolves the span, **materialises and renders each dir's lines before any prompt**, then streams through ScanPager.

**Wiring**: `pub(crate) type SharedFileRepo = Arc<dyn FileRepository + Send + Sync>` + `file_repository` field on AppServices (services.rs:26-70); params on `Runtime::new` (runtime.rs:44) and `bootstrap::build_runtime` (:229); bootstrap constructs the seeded in-memory repo. Compiler-driven fixes to ~10 AppServices test literals (session_driver.rs, menu_flow/{mod,pager,join,reply_forward,read_subprompt}.rs, tests/*_smoke.rs) — the shared-fixture-builder refactor is deliberately not bundled. ANSI written unconditionally (ColourTerminal strips SGR only and passes high-bit art bytes — colour_terminal.rs:32-52, telnet_listener.rs:130). No access gating this unit (H shown unconditionally; ACS gating deferred, noted).

**Telnet smoke** (`rust/tests/tierd_file_list_smoke.rs`, in-process per AGENTS.md point 6, cloned from tests/quickwins_smoke.rs:455-513): no `J` step needed — the corpus lives in the landing conference. Positionally asserts raw bytes between writes (SLICES.md wire checklist item 6): (1) `F 1` → exact 29-line first page (NextScan banner) → More? → `Q` → `Quit\r\n` + two `\x1b[0m\r\n` + menu; (2) `F 2` → the 3-frame trio incl. the shared-date no-separator boundary → post-End More? → `n` → echoed n → `Q` → `\x08 \x08Quit`; (3) bare `F` → `(1-2)` prompt → Enter → abort tail; (4) `F 99` → highest-dir error; (5) `F H` → HOLD `Nothing found!`; (6) `J 2` then `F 1` → `Nothing found!` (empty conference). The spawned-binary family smoke stays D-wire's (now smaller) job.

## 9. Provisional behaviours (tagged UNVERIFIED in COMMAND_PARITY.md, recapture wishlist)
`?` at More? (help + re-prompt); mid-list lone `n` (echo + clear + exit); ns-confirm answered `n` (resume paged); unknown More? keys (continue); `F A` with an empty middle dir; menu-level junk args incl. banner question; `F 0` → highest-dir error; A/U/H as prompt answers; counter reset at dir transitions; zero-area conference; H option for non-hold users; framed rendering of actual held files (seed has none; unit-test-only inference).

## 10. Docs + spec (definition of done, AGENTS.md pre-commit step 5)
- **slices/cmds-files-list.md** (already AquaScan-framed): D1 — three-variant FileStatus note (deferred variants + first writers), drop "uploader" from the adapter-surface bullet, replace "and the SQLite metadata store" with the D2s deferral; D2 — correct scope bullets (post-End `n` erase is deferred-to-next-key; `?` is UNVERIFIED; C/F/R read-and-discard pulled IN per the doc's delegation; NS token IN); add the **D2s** slice entry (contract in §5) and **D2b** (read_key); shrink D-wire's remit.
- **designs/FILES.md**: verify/finish the legacy_dir.rs removal + round-trip "neither" resolution; record D2s timing rationale + `file_storage` reservation.
- **COMMAND_PARITY.md**: new "Tier D — F file listings (NextScan vs AquaScan door)" section: b"..." capture quotes with transcript file:line, MATCH/COSMETIC (Enter-required pager, page positions ≥ page 3, three branding swaps with width math)/UNVERIFIED tags, the stock-internal diff record (LF-CR inversion, `(Pause)...(f)lags` prompt, lowercase getDirSpan prompt, express.e cites), live-wins rule.
- **specs/files.allium**: minimal ListFiles browse rule — requires `session.state = menu`; visible set **{available, lcfiles}** (consistent with FlagFile :165); per-area `uploaded_at` ascending; HOLD = held_for_review; upload area = highest number **phrased as the default of the existing `upload_area_for` blackbox** (:543-546); record `File.check` as a stored field; scope note that wire presentation follows the captured door UX with NextScan branding.
- **SYSTEM.md**: diagram, AppServices/MenuCommand tables, file_list module, files seeding section (mirroring :366-382), TerminalEcho::Silent.

## 11. Testing & mutants strategy
Strict red-green per step; focused `cargo mutants --file` after each step, full `cargo mutants` + `cargo nextest run` + warning-free `cargo build` + `cargo test --doc` before commit (rust/.cargo/mutants.toml → nextest). Planned killers: Bytes boundaries (u64::MAX, zero floor); both FileStatus filter arms constructed; frameability boundaries 12/13 chars and 9,999,999/10,000,000; 1-vs-2-digit pad (live in the seed — dir 1 reaches #23); date-group boundary (seeded Dir2 pair); every pager verb's exact bytes via CaptureTerminal scripts; seed mutants die on the smoke's positional asserts. Asset-reading tests keep the cargo-mutants early-return pattern (menu_command.rs:978-982). Expected-bytes for capture-segment tests are inlined (assetless-copy safe).