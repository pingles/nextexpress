//! File-backed [`ScreenRepository`] with in-memory caching.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tokio::sync::Mutex;

use crate::app::screens::{ScreenFuture, ScreenRepository};

/// Built-in fallback banner used when the configured `BBSTITLE.txt`
/// file is missing. Telnet line ending (CRLF) so it renders correctly
/// on clients that do not translate bare LF.
const FALLBACK_BANNER: &[u8] = b"NextExpress\r\n";

/// Built-in fallback menu used when the configured `Conf02/Menu.txt`
/// file is missing.
const FALLBACK_MENU: &[u8] = b"[ Default menu - type G to log off ]\r\n";

/// Built-in fallback NEWUSERPW screen used when the configured
/// `Screens/NEWUSERPW.txt` file is missing. Mirrors the spirit of the
/// legacy `AmiExpress` prompt (`amiexpress/express.e:30014`): a short
/// announcement that the user is now in the registration sub-flow.
const FALLBACK_NEW_USER_PW: &[u8] = b"\r\nNew user registration.\r\n";

/// Built-in fallback NONEWUSERS screen used when the configured
/// `Screens/NONEWUSERS.txt` file is missing. Rendered when
/// `core/config.allow_new_users = false`
/// (`amiexpress/express.e:30008`). One short line so the user knows
/// why the connection is closing.
const FALLBACK_NO_NEW_USERS: &[u8] = b"\r\nNew user registration is not available.\r\n";

/// Built-in fallback for `Screens/JoinConf.txt` — the screen shown
/// before the `J` command's `Conference Number (1-N): ` prompt
/// (`amiexpress/express.e:25143`, resolved at `:6588`). The reference
/// renders NOTHING when the screen file is absent (verified live
/// against `AmiExpress` 5.6.0,
/// `comparison/evidence-tierC/live-observations.md`), so the fallback
/// is empty: the screen appears only when the sysop installs the
/// asset.
///
/// `SCREEN_JOIN` / `SCREEN_JOINED` are intentionally absent: the
/// conference-join wire output is hardcoded inline in legacy
/// `joinConf` (`amiexpress/express.e:5071-5085`); the matching
/// `SCREEN_JOIN` / `SCREEN_JOINED` files belong to the new-user
/// registration flow (`:30057`, `:30125`).
const FALLBACK_JOINCONF: &[u8] = b"";

/// Built-in fallback for the `JoinMsgBase` screen pair — the screen
/// shown before the `Message Base Number (1-N): ` prompt
/// (`amiexpress/express.e:25221-25222`, resolved at `:6591-6596`).
/// The reference renders NOTHING when neither the conference-local
/// nor the node-level file is installed (verified live against
/// `AmiExpress` 5.6.0,
/// `comparison/evidence-tierC/live-observations.md`), so the
/// fallback is empty: the screen appears only when the sysop
/// installs an asset.
const FALLBACK_JOINMSGBASE: &[u8] = b"";

/// Built-in fallback for `Screens/REALNAMES.txt` (Slice 34,
/// `amiexpress/express.e:28169`). Rendered the first time a join
/// promotes the session into a real-name conference.
const FALLBACK_REALNAMES: &[u8] =
    b"\r\nThis conference uses real names. Your posts will be tagged with your legal name.\r\n";

/// Built-in fallback for `Screens/INTERNETNAMES.txt` (Slice 34,
/// `amiexpress/express.e:28169`). Rendered the first time a join
/// promotes the session into an internet-name conference.
const FALLBACK_INTERNETNAMES: &[u8] =
    b"\r\nThis conference uses internet names. Your posts will be tagged with your internet alias.\r\n";

/// Built-in fallback for `Screens/MAILSCAN.txt` (Slice 41,
/// `amiexpress/axenums.e:19`). Rendered immediately before the
/// auto-scan-on-join summary when the user has unread mail.
const FALLBACK_MAILSCAN: &[u8] = b"\r\n*** New mail in this conference ***\r\n";

/// Built-in fallback for `Screens/LOGOFF.txt`
/// (`amiexpress/express.e:6554, :8187`). The legacy screen is
/// sysop-supplied so there is no canonical text to fall back to;
/// callers already write a dedicated `Goodbye!` line after the
/// screen, so the fallback is empty — an absent asset means "no
/// pre-goodbye splash."
const FALLBACK_LOGOFF: &[u8] = b"";

/// Built-in fallback for `<bbs-loc>/BBSHelp.txt`
/// (`amiexpress/express.e:25079`). The `H` command's caller writes
/// the dedicated `Sorry Help is unavailable at this time.` line
/// (`HELP_UNAVAILABLE_LINE`) when the
/// returned bytes are empty, so the fallback here is empty.
const FALLBACK_BBS_HELP: &[u8] = b"";

/// Lower bound for the security-level menu walk, mirroring the
/// `minLevel := 5` default in `amiexpress/express.e:6246`
/// (findSecurityScreen).
const MIN_SECURITY_LEVEL: u8 = 5;

/// File-backed screen repository rooted at a BBS installation path.
#[derive(Debug)]
pub struct FileScreenRepository {
    bbs_path: PathBuf,
    banner: Mutex<Option<Vec<u8>>>,
    default_menu: Mutex<HashMap<u8, Vec<u8>>>,
    /// Per-(`conference_number`, `access_level`) cache for
    /// [`ScreenRepository::conference_menu`] (Slice 31). Filled
    /// lazily on first lookup so the cache mirrors what was actually
    /// asked for.
    conference_menu: Mutex<HashMap<(u32, u8), Vec<u8>>>,
    new_user_password: Mutex<Option<Vec<u8>>>,
    no_new_users: Mutex<Option<Vec<u8>>>,
    joinconf: Mutex<Option<Vec<u8>>>,
    /// Per-conference cache for
    /// [`ScreenRepository::joinmsgbase_screen`] (Tier C C4b): the
    /// resolution is conference-local-first, so the bytes differ per
    /// conference. Filled lazily on first lookup, like
    /// [`Self::conference_menu`].
    joinmsgbase: Mutex<HashMap<u32, Vec<u8>>>,
    realnames: Mutex<Option<Vec<u8>>>,
    internetnames: Mutex<Option<Vec<u8>>>,
    mailscan: Mutex<Option<Vec<u8>>>,
    logoff: Mutex<Option<Vec<u8>>>,
    bbs_help: Mutex<Option<Vec<u8>>>,
}

impl FileScreenRepository {
    /// Constructs a repository rooted at `bbs_path`.
    #[must_use]
    pub fn new(bbs_path: PathBuf) -> Self {
        Self {
            bbs_path,
            banner: Mutex::new(None),
            default_menu: Mutex::new(HashMap::new()),
            conference_menu: Mutex::new(HashMap::new()),
            new_user_password: Mutex::new(None),
            no_new_users: Mutex::new(None),
            joinconf: Mutex::new(None),
            joinmsgbase: Mutex::new(HashMap::new()),
            realnames: Mutex::new(None),
            internetnames: Mutex::new(None),
            mailscan: Mutex::new(None),
            logoff: Mutex::new(None),
            bbs_help: Mutex::new(None),
        }
    }

    async fn cached_file(
        &self,
        cache: &Mutex<Option<Vec<u8>>>,
        path: &Path,
        fallback: &[u8],
    ) -> Vec<u8> {
        if let Some(bytes) = cache.lock().await.as_ref().cloned() {
            return bytes;
        }

        let loaded = match tokio::fs::read(path).await {
            Ok(bytes) => normalise_to_crlf(&bytes),
            Err(_) => fallback.to_vec(),
        };

        let mut cached = cache.lock().await;
        if let Some(bytes) = cached.as_ref() {
            return bytes.clone();
        }
        *cached = Some(loaded.clone());
        loaded
    }

    async fn banner_bytes(&self) -> Vec<u8> {
        let path = self.bbs_path.join("Screens").join("BBSTITLE.txt");
        self.cached_file(&self.banner, &path, FALLBACK_BANNER).await
    }

    async fn default_menu_bytes(&self, access_level: u8) -> Vec<u8> {
        if let Some(bytes) = self.default_menu.lock().await.get(&access_level).cloned() {
            return bytes;
        }
        let loaded = self.resolve_default_menu(access_level).await;
        let mut cached = self.default_menu.lock().await;
        cached
            .entry(access_level)
            .or_insert_with(|| loaded.clone())
            .clone()
    }

    /// Walks the legacy security-level menu lookup and returns the
    /// bytes of the first matching file, falling back to the plain
    /// `Conf02/Menu.txt` and ultimately to [`FALLBACK_MENU`].
    async fn resolve_default_menu(&self, access_level: u8) -> Vec<u8> {
        let conf_dir = self.bbs_path.join("Conf02");
        let mut sec_level = (access_level / 5) * 5;
        while sec_level >= MIN_SECURITY_LEVEL {
            let path = conf_dir.join(format!("Menu{sec_level}.txt"));
            if let Ok(bytes) = tokio::fs::read(&path).await {
                return normalise_to_crlf(&bytes);
            }
            sec_level -= 5;
        }
        let plain = conf_dir.join("Menu.txt");
        if let Ok(bytes) = tokio::fs::read(&plain).await {
            return normalise_to_crlf(&bytes);
        }
        FALLBACK_MENU.to_vec()
    }

    async fn conference_menu_bytes(&self, conference_number: u32, access_level: u8) -> Vec<u8> {
        if let Some(bytes) = self
            .conference_menu
            .lock()
            .await
            .get(&(conference_number, access_level))
            .cloned()
        {
            return bytes;
        }
        let loaded = match self
            .resolve_conference_menu(conference_number, access_level)
            .await
        {
            Some(bytes) => bytes,
            None => self.default_menu_bytes(access_level).await,
        };
        let mut cached = self.conference_menu.lock().await;
        cached
            .entry((conference_number, access_level))
            .or_insert_with(|| loaded.clone())
            .clone()
    }

    /// Walks the per-conference menu lookup, returning `None` when no
    /// asset is on disk so the caller can fall through to the
    /// system-wide [`Self::default_menu_bytes`]. Mirrors the
    /// security-level walk in [`Self::resolve_default_menu`] but
    /// rooted at `Conf<NN>` instead of the hard-coded `Conf02`.
    async fn resolve_conference_menu(
        &self,
        conference_number: u32,
        access_level: u8,
    ) -> Option<Vec<u8>> {
        let conf_dir = self.bbs_path.join(format!("Conf{conference_number:02}"));
        let mut sec_level = (access_level / 5) * 5;
        while sec_level >= MIN_SECURITY_LEVEL {
            let path = conf_dir.join(format!("Menu{sec_level}.txt"));
            if let Ok(bytes) = tokio::fs::read(&path).await {
                return Some(normalise_to_crlf(&bytes));
            }
            sec_level -= 5;
        }
        for candidate in ["menu.txt", "Menu.txt"] {
            let path = conf_dir.join(candidate);
            if let Ok(bytes) = tokio::fs::read(&path).await {
                return Some(normalise_to_crlf(&bytes));
            }
        }
        None
    }

    async fn new_user_password_bytes(&self) -> Vec<u8> {
        let path = self.bbs_path.join("Screens").join("NEWUSERPW.txt");
        self.cached_file(&self.new_user_password, &path, FALLBACK_NEW_USER_PW)
            .await
    }

    async fn no_new_users_bytes(&self) -> Vec<u8> {
        let path = self.bbs_path.join("Screens").join("NONEWUSERS.txt");
        self.cached_file(&self.no_new_users, &path, FALLBACK_NO_NEW_USERS)
            .await
    }

    async fn joinconf_bytes(&self) -> Vec<u8> {
        let path = self.bbs_path.join("Screens").join("JoinConf.txt");
        self.cached_file(&self.joinconf, &path, FALLBACK_JOINCONF)
            .await
    }

    async fn joinmsgbase_bytes(&self, conference_number: u32) -> Vec<u8> {
        if let Some(bytes) = self
            .joinmsgbase
            .lock()
            .await
            .get(&conference_number)
            .cloned()
        {
            return bytes;
        }
        let loaded = self.resolve_joinmsgbase(conference_number).await;
        let mut cached = self.joinmsgbase.lock().await;
        cached
            .entry(conference_number)
            .or_insert_with(|| loaded.clone())
            .clone()
    }

    /// Walks the legacy `JoinMsgBase` screen lookup
    /// (`amiexpress/express.e:25221-25222`): the conference-local
    /// `Conf<NN>/JoinMsgBase.txt` (`SCREEN_CONF_JOINMSGBASE`,
    /// `:6592`) wins over the node-level `Screens/JoinMsgBase.txt`
    /// (`SCREEN_JOINMSGBASE`, `:6595`); with neither installed the
    /// result is [`FALLBACK_JOINMSGBASE`] (empty — nothing precedes
    /// the prompt, as on the reference).
    async fn resolve_joinmsgbase(&self, conference_number: u32) -> Vec<u8> {
        let candidates = [
            self.bbs_path
                .join(format!("Conf{conference_number:02}"))
                .join("JoinMsgBase.txt"),
            self.bbs_path.join("Screens").join("JoinMsgBase.txt"),
        ];
        for path in candidates {
            if let Ok(bytes) = tokio::fs::read(&path).await {
                return normalise_to_crlf(&bytes);
            }
        }
        FALLBACK_JOINMSGBASE.to_vec()
    }

    async fn realnames_bytes(&self) -> Vec<u8> {
        let path = self.bbs_path.join("Screens").join("REALNAMES.txt");
        self.cached_file(&self.realnames, &path, FALLBACK_REALNAMES)
            .await
    }

    async fn internetnames_bytes(&self) -> Vec<u8> {
        let path = self.bbs_path.join("Screens").join("INTERNETNAMES.txt");
        self.cached_file(&self.internetnames, &path, FALLBACK_INTERNETNAMES)
            .await
    }

    async fn mailscan_bytes(&self) -> Vec<u8> {
        let path = self.bbs_path.join("Screens").join("MAILSCAN.txt");
        self.cached_file(&self.mailscan, &path, FALLBACK_MAILSCAN)
            .await
    }

    async fn logoff_bytes(&self) -> Vec<u8> {
        let path = self.bbs_path.join("Screens").join("LOGOFF.txt");
        self.cached_file(&self.logoff, &path, FALLBACK_LOGOFF).await
    }

    async fn bbs_help_bytes(&self) -> Vec<u8> {
        let path = self.bbs_path.join("BBSHelp.txt");
        self.cached_file(&self.bbs_help, &path, FALLBACK_BBS_HELP)
            .await
    }

    /// Resolves `<bbs-loc>/help/<topic>.txt`, stripping trailing
    /// characters from `topic` and retrying until a screen is found —
    /// the legacy `internalCommandUpHat` truncate-and-retry loop
    /// (`amiexpress/express.e:25094-25107`). Returns empty bytes when
    /// no prefix matches. Not cached: `^` is infrequent and the topic
    /// varies per call, so each lookup reads from disk like the legacy.
    async fn topic_help_bytes(&self, topic: &str) -> Vec<u8> {
        if !is_safe_topic_help_name(topic) {
            return Vec::new();
        }
        let help_dir = self.bbs_path.join("help");
        // Longest-first: try the full topic, then progressively shorter
        // prefixes, so the most specific screen wins (`^FILES` prefers
        // `FILES.txt` over `FILE.txt`). `get(..len)` yields `None` on a
        // non-UTF-8 char boundary, which is simply skipped.
        for len in (1..=topic.len()).rev() {
            let Some(candidate) = topic.get(..len) else {
                continue;
            };
            let path = help_dir.join(format!("{candidate}.txt"));
            if let Ok(bytes) = tokio::fs::read(&path).await {
                return normalise_to_crlf(&bytes);
            }
        }
        Vec::new()
    }
}

fn is_safe_topic_help_name(topic: &str) -> bool {
    !topic.is_empty()
        && topic
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

impl ScreenRepository for FileScreenRepository {
    fn banner(&self) -> ScreenFuture<'_> {
        Box::pin(async move { self.banner_bytes().await })
    }

    fn default_menu(&self, access_level: u8) -> ScreenFuture<'_> {
        Box::pin(async move { self.default_menu_bytes(access_level).await })
    }

    fn conference_menu(&self, conference_number: u32, access_level: u8) -> ScreenFuture<'_> {
        Box::pin(async move {
            self.conference_menu_bytes(conference_number, access_level)
                .await
        })
    }

    fn new_user_password(&self) -> ScreenFuture<'_> {
        Box::pin(async move { self.new_user_password_bytes().await })
    }

    fn no_new_users(&self) -> ScreenFuture<'_> {
        Box::pin(async move { self.no_new_users_bytes().await })
    }

    fn joinconf_screen(&self) -> ScreenFuture<'_> {
        Box::pin(async move { self.joinconf_bytes().await })
    }

    fn joinmsgbase_screen(&self, conference_number: u32) -> ScreenFuture<'_> {
        Box::pin(async move { self.joinmsgbase_bytes(conference_number).await })
    }

    fn realnames_screen(&self) -> ScreenFuture<'_> {
        Box::pin(async move { self.realnames_bytes().await })
    }

    fn internetnames_screen(&self) -> ScreenFuture<'_> {
        Box::pin(async move { self.internetnames_bytes().await })
    }

    fn mailscan_screen(&self) -> ScreenFuture<'_> {
        Box::pin(async move { self.mailscan_bytes().await })
    }

    fn logoff_screen(&self) -> ScreenFuture<'_> {
        Box::pin(async move { self.logoff_bytes().await })
    }

    fn bbs_help_screen(&self) -> ScreenFuture<'_> {
        Box::pin(async move { self.bbs_help_bytes().await })
    }

    fn topic_help(&self, topic: &str) -> ScreenFuture<'_> {
        let topic = topic.to_owned();
        Box::pin(async move { self.topic_help_bytes(&topic).await })
    }
}

/// Normalises every line-ending convention found in disk-loaded screen
/// files to telnet `\r\n`, satisfying the wire-quality CRLF discipline
/// (`SLICES.md` adapter checklist item 3): Amiga `\b\n`, Unix `\n`,
/// classic-Mac `\r`, and existing `\r\n` all emit a single `\r\n`.
/// Other bytes, including mid-line `\b` (ANSI BS) and ANSI escapes,
/// pass through unchanged.
fn normalise_to_crlf(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut bytes = input.iter().copied().peekable();
    while let Some(byte) = bytes.next() {
        match byte {
            0x08 | b'\r' if bytes.peek() == Some(&b'\n') => {
                bytes.next();
                out.extend_from_slice(b"\r\n");
            }
            b'\r' | b'\n' => out.extend_from_slice(b"\r\n"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_to_crlf_replaces_bs_lf() {
        assert_eq!(normalise_to_crlf(b"foo\x08\nbar"), b"foo\r\nbar");
    }

    #[test]
    fn normalise_to_crlf_preserves_ansi_escapes() {
        let ansi = b"\x1b[31mRED\x1b[0m\x08\n";
        assert_eq!(normalise_to_crlf(ansi), b"\x1b[31mRED\x1b[0m\r\n");
    }

    #[test]
    fn normalise_to_crlf_leaves_other_bytes_alone() {
        assert_eq!(normalise_to_crlf(b"hello"), b"hello");
        assert_eq!(normalise_to_crlf(b"a\x08b"), b"a\x08b");
    }

    #[test]
    fn normalise_to_crlf_promotes_bare_lf_to_crlf() {
        assert_eq!(normalise_to_crlf(b"foo\nbar"), b"foo\r\nbar");
    }

    #[test]
    fn normalise_to_crlf_leaves_existing_crlf_unchanged() {
        assert_eq!(normalise_to_crlf(b"foo\r\nbar"), b"foo\r\nbar");
    }

    #[test]
    fn normalise_to_crlf_promotes_bare_cr_to_crlf() {
        assert_eq!(normalise_to_crlf(b"foo\rbar"), b"foo\r\nbar");
    }

    #[test]
    fn normalise_to_crlf_normalises_mixed_input() {
        let mixed = b"a\nb\r\nc\x08\nd\re";
        assert_eq!(normalise_to_crlf(mixed), b"a\r\nb\r\nc\r\nd\r\ne");
    }

    // Boundary: `\x08` as the *first* byte. The two-byte look-ahead
    // arm has to handle i=0 without underflowing the index check
    // (`i + 1 < input.len()` must use addition, not subtraction).
    #[test]
    fn normalise_to_crlf_handles_bs_lf_at_start() {
        assert_eq!(normalise_to_crlf(b"\x08\n"), b"\r\n");
    }

    // Boundary: `\r` as the *first* byte followed by `\n`. Same
    // index-arithmetic invariant as the BS/LF start case.
    #[test]
    fn normalise_to_crlf_handles_crlf_at_start() {
        assert_eq!(normalise_to_crlf(b"\r\nrest"), b"\r\nrest");
    }

    // Boundary: lone `\x08` as the *last* byte. The look-ahead
    // guard must be a strict `<` so we never read past the buffer;
    // a `<=` mutant would index `input[i + 1]` out of bounds and
    // panic.
    #[test]
    fn normalise_to_crlf_handles_trailing_bs() {
        assert_eq!(normalise_to_crlf(b"foo\x08"), b"foo\x08");
    }

    // Boundary: lone `\r` as the *last* byte. The bare-CR arm
    // promotes it to `\r\n` — the look-ahead arm must *not* fire.
    #[test]
    fn normalise_to_crlf_handles_trailing_bare_cr() {
        assert_eq!(normalise_to_crlf(b"foo\r"), b"foo\r\n");
    }

    // Boundary: lone `\n` as the only byte. Catches `i += 1`
    // mutants in the bare-LF arm at i=0 (underflow panic) and
    // pins down the empty-prefix output.
    #[test]
    fn normalise_to_crlf_promotes_solo_lf() {
        assert_eq!(normalise_to_crlf(b"\n"), b"\r\n");
    }

    #[tokio::test]
    async fn banner_is_cached_after_first_load() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Screens")).unwrap();
        let path = dir.path().join("Screens").join("BBSTITLE.txt");
        std::fs::write(&path, b"FIRST\x08\n").unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());

        assert_eq!(repo.banner().await, b"FIRST\r\n");
        std::fs::write(&path, b"SECOND\x08\n").unwrap();
        assert_eq!(repo.banner().await, b"FIRST\r\n");
    }

    #[tokio::test]
    async fn new_user_password_returns_fallback_when_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.new_user_password().await, FALLBACK_NEW_USER_PW);
    }

    #[tokio::test]
    async fn new_user_password_loads_from_disk_when_present() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Screens")).unwrap();
        std::fs::write(
            dir.path().join("Screens").join("NEWUSERPW.txt"),
            b"WELCOME NEW USER\x08\n",
        )
        .unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.new_user_password().await, b"WELCOME NEW USER\r\n");
    }

    #[tokio::test]
    async fn no_new_users_returns_fallback_when_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.no_new_users().await, FALLBACK_NO_NEW_USERS);
    }

    #[tokio::test]
    async fn default_menu_is_cached_per_access_level_after_first_load() {
        // The first load reads the file; subsequent loads at the same
        // level return the cached bytes even if the file is rewritten,
        // matching the banner-cache contract.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Conf02")).unwrap();
        let path = dir.path().join("Conf02").join("Menu5.txt");
        std::fs::write(&path, b"FIRST\n").unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.default_menu(5).await, b"FIRST\r\n");
        std::fs::write(&path, b"SECOND\n").unwrap();
        assert_eq!(repo.default_menu(5).await, b"FIRST\r\n");
    }

    #[tokio::test]
    async fn default_menu_returns_fallback_when_no_menu_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.default_menu(50).await, FALLBACK_MENU);
    }

    #[tokio::test]
    async fn default_menu_falls_back_to_plain_menu_for_low_access_level() {
        // access_level 2 floors to secLevel 0, below the legacy
        // minLevel of 5, so the security-level search is skipped and
        // the plain Menu.txt is used (mirrors
        // `amiexpress/express.e:6246` findSecurityScreen).
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Conf02")).unwrap();
        std::fs::write(dir.path().join("Conf02").join("Menu.txt"), b"PLAIN\x08\n").unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.default_menu(2).await, b"PLAIN\r\n");
    }

    #[tokio::test]
    async fn default_menu_picks_security_level_file_in_preference_to_plain_menu() {
        // access_level 5 resolves to secLevel 5, so Menu5.txt is
        // picked over Menu.txt.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Conf02")).unwrap();
        std::fs::write(dir.path().join("Conf02").join("Menu.txt"), b"PLAIN\n").unwrap();
        std::fs::write(dir.path().join("Conf02").join("Menu5.txt"), b"FIVE\n").unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.default_menu(5).await, b"FIVE\r\n");
    }

    #[tokio::test]
    async fn default_menu_walks_down_in_steps_of_five_to_find_a_security_file() {
        // access_level 12 floors to secLevel 10. With only Menu5.txt
        // present the walk descends 10 -> 5 and resolves to it.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Conf02")).unwrap();
        std::fs::write(dir.path().join("Conf02").join("Menu5.txt"), b"FIVE\n").unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.default_menu(12).await, b"FIVE\r\n");
    }

    #[tokio::test]
    async fn default_menu_prefers_higher_security_level_when_multiple_match() {
        // access_level 20 floors to secLevel 20. Both Menu20.txt and
        // Menu5.txt are present; the higher one wins (the walk visits
        // 20 first).
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Conf02")).unwrap();
        std::fs::write(dir.path().join("Conf02").join("Menu5.txt"), b"FIVE\n").unwrap();
        std::fs::write(dir.path().join("Conf02").join("Menu20.txt"), b"TWENTY\n").unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.default_menu(20).await, b"TWENTY\r\n");
    }

    #[tokio::test]
    async fn no_new_users_loads_from_disk_when_present() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Screens")).unwrap();
        std::fs::write(
            dir.path().join("Screens").join("NONEWUSERS.txt"),
            b"ACCESS DENIED\x08\n",
        )
        .unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.no_new_users().await, b"ACCESS DENIED\r\n");
    }

    #[tokio::test]
    async fn conference_menu_loads_per_conference_lowercase_menu_file() {
        // Slice 31: prefer `Conf<n>/menu.txt` over the system-wide
        // fallback. The legacy seed ships a lowercase
        // `defaultbbs/Conf01/menu.txt`.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Conf03")).unwrap();
        std::fs::write(dir.path().join("Conf03").join("menu.txt"), b"CONF3\n").unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.conference_menu(3, 50).await, b"CONF3\r\n");
    }

    #[tokio::test]
    async fn conference_menu_falls_back_to_system_wide_default_menu() {
        // No per-conference asset; the lookup walks all the way
        // through to `default_menu`, which itself falls back to
        // `Conf02/Menu.txt`.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Conf02")).unwrap();
        std::fs::write(dir.path().join("Conf02").join("Menu.txt"), b"DEFAULT\n").unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.conference_menu(7, 50).await, b"DEFAULT\r\n");
    }

    #[tokio::test]
    async fn conference_menu_prefers_security_level_file_inside_conference_dir() {
        // access_level 20 floors to secLevel 20. With both Menu20.txt
        // and the plain menu.txt present, the security file wins.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Conf01")).unwrap();
        std::fs::write(dir.path().join("Conf01").join("menu.txt"), b"PLAIN\n").unwrap();
        std::fs::write(dir.path().join("Conf01").join("Menu20.txt"), b"TWENTY\n").unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.conference_menu(1, 20).await, b"TWENTY\r\n");
    }

    #[tokio::test]
    async fn conference_menu_walks_security_levels_in_steps_of_five_within_conf_dir() {
        // access_level 20 floors to secLevel 20. With only Menu5.txt
        // present in Conf01, the walk descends 20 -> 15 -> 10 -> 5
        // and finds it. Pinning the FIVE result here closes a
        // mutation-test gap on the `sec_level -= 5` step.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Conf01")).unwrap();
        std::fs::write(dir.path().join("Conf01").join("Menu5.txt"), b"FIVE\n").unwrap();
        std::fs::write(dir.path().join("Conf01").join("menu.txt"), b"PLAIN\n").unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.conference_menu(1, 20).await, b"FIVE\r\n");
    }

    #[tokio::test]
    async fn joinconf_screen_loads_from_disk_when_present() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Screens")).unwrap();
        std::fs::write(
            dir.path().join("Screens").join("JoinConf.txt"),
            b"PICK A CONF\x08\n",
        )
        .unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.joinconf_screen().await, b"PICK A CONF\r\n");
    }

    #[tokio::test]
    async fn joinmsgbase_screen_prefers_the_conference_local_asset() {
        // SCREEN_CONF_JOINMSGBASE resolves first
        // (`amiexpress/express.e:25221`, file lookup `:6592`): the
        // conference-local `Conf<NN>/JoinMsgBase.txt` wins over the
        // node-level `Screens/JoinMsgBase.txt`.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Conf03")).unwrap();
        std::fs::create_dir_all(dir.path().join("Screens")).unwrap();
        std::fs::write(
            dir.path().join("Conf03").join("JoinMsgBase.txt"),
            b"CONF LOCAL\x08\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("Screens").join("JoinMsgBase.txt"),
            b"NODE LEVEL\x08\n",
        )
        .unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.joinmsgbase_screen(3).await, b"CONF LOCAL\r\n");
    }

    #[tokio::test]
    async fn joinmsgbase_screen_falls_back_to_the_node_level_asset() {
        // With no conference-local file the legacy falls back to
        // SCREEN_JOINMSGBASE in the node screen dir
        // (`amiexpress/express.e:25222`, file lookup `:6595`).
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Screens")).unwrap();
        std::fs::write(
            dir.path().join("Screens").join("JoinMsgBase.txt"),
            b"NODE LEVEL\x08\n",
        )
        .unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.joinmsgbase_screen(3).await, b"NODE LEVEL\r\n");
    }

    #[tokio::test]
    async fn joinmsgbase_screen_returns_empty_bytes_when_no_asset_exists() {
        // The AmiExpress 5.6.0 reference shows NOTHING before the
        // `Message Base Number (1-N): ` prompt when neither screen
        // file is installed — the fallback must be empty so the
        // caller writes nothing.
        let dir = tempfile::tempdir().unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert!(repo.joinmsgbase_screen(1).await.is_empty());
    }

    #[tokio::test]
    async fn joinmsgbase_screen_caches_per_conference() {
        // The cache is keyed by conference number: conference 1's
        // local asset must not leak into conference 2's lookup.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Conf01")).unwrap();
        std::fs::write(
            dir.path().join("Conf01").join("JoinMsgBase.txt"),
            b"ONE\x08\n",
        )
        .unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.joinmsgbase_screen(1).await, b"ONE\r\n");
        assert!(repo.joinmsgbase_screen(2).await.is_empty());
        // Second read of conference 1 is served from cache even after
        // the file is gone.
        std::fs::remove_file(dir.path().join("Conf01").join("JoinMsgBase.txt")).unwrap();
        assert_eq!(repo.joinmsgbase_screen(1).await, b"ONE\r\n");
    }

    #[tokio::test]
    async fn realnames_screen_falls_back_when_asset_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.realnames_screen().await, FALLBACK_REALNAMES);
    }

    #[tokio::test]
    async fn realnames_screen_loads_from_disk_when_present() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Screens")).unwrap();
        std::fs::write(
            dir.path().join("Screens").join("REALNAMES.txt"),
            b"REAL\x08\n",
        )
        .unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.realnames_screen().await, b"REAL\r\n");
    }

    #[tokio::test]
    async fn internetnames_screen_loads_from_disk_when_present() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Screens")).unwrap();
        std::fs::write(
            dir.path().join("Screens").join("INTERNETNAMES.txt"),
            b"INET\x08\n",
        )
        .unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.internetnames_screen().await, b"INET\r\n");
    }

    #[tokio::test]
    async fn internetnames_screen_falls_back_when_asset_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.internetnames_screen().await, FALLBACK_INTERNETNAMES);
    }

    #[tokio::test]
    async fn joinconf_returns_empty_bytes_when_asset_is_missing() {
        // The AmiExpress 5.6.0 reference shows NOTHING before the
        // `Conference Number (1-N): ` prompt when no JoinConf screen
        // file is installed — the fallback must be empty so the
        // caller writes nothing.
        let dir = tempfile::tempdir().unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.joinconf_screen().await, FALLBACK_JOINCONF);
        assert!(repo.joinconf_screen().await.is_empty());
    }

    #[tokio::test]
    async fn logoff_screen_returns_empty_fallback_when_asset_is_missing() {
        // Legacy SCREEN_LOGOFF is sysop-supplied. Absent file means
        // no pre-goodbye splash — caller writes the dedicated
        // Goodbye! line afterwards regardless.
        let dir = tempfile::tempdir().unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.logoff_screen().await, FALLBACK_LOGOFF);
        assert!(repo.logoff_screen().await.is_empty());
    }

    #[tokio::test]
    async fn logoff_screen_loads_from_disk_when_present() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Screens")).unwrap();
        std::fs::write(
            dir.path().join("Screens").join("LOGOFF.txt"),
            b"SEE YOU NEXT TIME\x08\n",
        )
        .unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.logoff_screen().await, b"SEE YOU NEXT TIME\r\n");
    }

    #[tokio::test]
    async fn logoff_screen_is_cached_after_first_load() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Screens")).unwrap();
        let path = dir.path().join("Screens").join("LOGOFF.txt");
        std::fs::write(&path, b"FIRST\n").unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.logoff_screen().await, b"FIRST\r\n");
        std::fs::write(&path, b"SECOND\n").unwrap();
        assert_eq!(repo.logoff_screen().await, b"FIRST\r\n");
    }

    #[tokio::test]
    async fn bbs_help_screen_returns_empty_fallback_when_asset_is_missing() {
        // Tier A quickwin A5: absent asset means the `H` command's
        // caller writes the dedicated `Sorry Help is unavailable at
        // this time.` line instead. The adapter signals absence with
        // empty bytes.
        let dir = tempfile::tempdir().unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.bbs_help_screen().await, FALLBACK_BBS_HELP);
        assert!(repo.bbs_help_screen().await.is_empty());
    }

    #[tokio::test]
    async fn bbs_help_screen_loads_from_disk_when_present() {
        // Per the slice, the BBSHelp asset lives at the BBS root
        // (matching legacy `<bbs-loc>/BBSHelp.txt`), not under
        // `Screens/`. Amiga `\b\n` line endings are translated to
        // telnet `\r\n` on the way out.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("BBSHelp.txt"),
            b"== Help ==\x08\nType G to log off.\x08\n",
        )
        .unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(
            repo.bbs_help_screen().await,
            b"== Help ==\r\nType G to log off.\r\n"
        );
    }

    #[tokio::test]
    async fn bbs_help_screen_is_cached_after_first_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("BBSHelp.txt");
        std::fs::write(&path, b"FIRST\n").unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.bbs_help_screen().await, b"FIRST\r\n");
        std::fs::write(&path, b"SECOND\n").unwrap();
        assert_eq!(repo.bbs_help_screen().await, b"FIRST\r\n");
    }

    #[tokio::test]
    async fn topic_help_loads_exact_match_from_help_dir() {
        // Tier A quickwin A10: `^NET` reads `<bbs-loc>/help/NET.txt`
        // (legacy `<bbs-loc>help/<params>`, `amiexpress/express.e:25094`).
        // Amiga `\b\n` line endings translate to telnet `\r\n`.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("help")).unwrap();
        std::fs::write(dir.path().join("help").join("NET.txt"), b"Net rules.\x08\n").unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.topic_help("NET").await, b"Net rules.\r\n");
    }

    #[tokio::test]
    async fn topic_help_truncates_topic_until_a_screen_matches() {
        // Legacy truncate-and-retry (`amiexpress/express.e:25102`):
        // `^FILES` with only `help/FIL.txt` on disk strips characters
        // (FILES -> FILE -> FIL) until a screen is found.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("help")).unwrap();
        std::fs::write(dir.path().join("help").join("FIL.txt"), b"Files help.\n").unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.topic_help("FILES").await, b"Files help.\r\n");
    }

    #[tokio::test]
    async fn topic_help_prefers_the_longest_matching_prefix() {
        // `^FILES` with both `FILE.txt` and `FILES.txt` present picks
        // the exact (longest) match, not a shorter prefix — the legacy
        // loop starts from the full topic.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("help")).unwrap();
        std::fs::write(dir.path().join("help").join("FILE.txt"), b"short\n").unwrap();
        std::fs::write(dir.path().join("help").join("FILES.txt"), b"long\n").unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.topic_help("FILES").await, b"long\r\n");
    }

    #[tokio::test]
    async fn topic_help_returns_empty_when_no_prefix_matches() {
        // No `help/` screen matches any prefix of the topic, so the
        // adapter signals "nothing to show" with empty bytes and the
        // `^` command is a silent no-op (`amiexpress/express.e:25105`).
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("help")).unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert!(repo.topic_help("XYZ").await.is_empty());
        // A bare `^` carries no topic — also empty.
        assert!(repo.topic_help("").await.is_empty());
    }

    #[tokio::test]
    async fn topic_help_rejects_parent_directory_traversal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("help")).unwrap();
        std::fs::write(dir.path().join("SECRET.txt"), b"secret\n").unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());

        assert!(repo.topic_help("../SECRET").await.is_empty());
    }

    #[tokio::test]
    async fn topic_help_rejects_path_separators_instead_of_truncating_to_safe_prefix() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("help")).unwrap();
        std::fs::write(dir.path().join("help").join("NET.txt"), b"net\n").unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());

        assert!(repo.topic_help("NET/../SECRET").await.is_empty());
    }

    #[tokio::test]
    async fn topic_help_rejects_absolute_path_topics() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("help")).unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());

        assert!(repo.topic_help("/tmp/SECRET").await.is_empty());
    }

    #[tokio::test]
    async fn conference_menu_caches_loaded_bytes_per_conference_and_level() {
        // Verifies the (conference_number, access_level) cache: a
        // rewrite after first access doesn't change the served
        // bytes.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Conf01")).unwrap();
        let path = dir.path().join("Conf01").join("menu.txt");
        std::fs::write(&path, b"FIRST\n").unwrap();
        let repo = FileScreenRepository::new(dir.path().to_path_buf());
        assert_eq!(repo.conference_menu(1, 50).await, b"FIRST\r\n");
        std::fs::write(&path, b"SECOND\n").unwrap();
        assert_eq!(repo.conference_menu(1, 50).await, b"FIRST\r\n");
    }
}
