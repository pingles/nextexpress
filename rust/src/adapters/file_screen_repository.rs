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
    new_user_password: Mutex<Option<Vec<u8>>>,
    no_new_users: Mutex<Option<Vec<u8>>>,
}

impl FileScreenRepository {
    /// Constructs a repository rooted at `bbs_path`.
    #[must_use]
    pub fn new(bbs_path: PathBuf) -> Self {
        Self {
            bbs_path,
            banner: Mutex::new(None),
            default_menu: Mutex::new(HashMap::new()),
            new_user_password: Mutex::new(None),
            no_new_users: Mutex::new(None),
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
}

impl ScreenRepository for FileScreenRepository {
    fn banner(&self) -> ScreenFuture<'_> {
        Box::pin(async move { self.banner_bytes().await })
    }

    fn default_menu(&self, access_level: u8) -> ScreenFuture<'_> {
        Box::pin(async move { self.default_menu_bytes(access_level).await })
    }

    fn new_user_password(&self) -> ScreenFuture<'_> {
        Box::pin(async move { self.new_user_password_bytes().await })
    }

    fn no_new_users(&self) -> ScreenFuture<'_> {
        Box::pin(async move { self.no_new_users_bytes().await })
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
    let mut i = 0;
    while i < input.len() {
        match input[i] {
            0x08 | b'\r' if i + 1 < input.len() && input[i + 1] == b'\n' => {
                out.extend_from_slice(b"\r\n");
                i += 2;
            }
            b'\r' | b'\n' => {
                out.extend_from_slice(b"\r\n");
                i += 1;
            }
            other => {
                out.push(other);
                i += 1;
            }
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
}
