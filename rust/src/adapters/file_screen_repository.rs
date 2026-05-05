//! File-backed [`ScreenRepository`] with in-memory caching.

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

/// File-backed screen repository rooted at a BBS installation path.
#[derive(Debug)]
pub struct FileScreenRepository {
    bbs_path: PathBuf,
    banner: Mutex<Option<Vec<u8>>>,
    default_menu: Mutex<Option<Vec<u8>>>,
}

impl FileScreenRepository {
    /// Constructs a repository rooted at `bbs_path`.
    pub fn new(bbs_path: PathBuf) -> Self {
        Self {
            bbs_path,
            banner: Mutex::new(None),
            default_menu: Mutex::new(None),
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
            Ok(bytes) => translate_amiga_line_endings(&bytes),
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

    async fn default_menu_bytes(&self) -> Vec<u8> {
        let path = self.bbs_path.join("Conf02").join("Menu.txt");
        self.cached_file(&self.default_menu, &path, FALLBACK_MENU)
            .await
    }
}

impl ScreenRepository for FileScreenRepository {
    fn banner(&self) -> ScreenFuture<'_> {
        Box::pin(async move { self.banner_bytes().await })
    }

    fn default_menu(&self) -> ScreenFuture<'_> {
        Box::pin(async move { self.default_menu_bytes().await })
    }
}

/// Replaces the Amiga `\b\n` (BS+LF) sequence with the telnet `\r\n`
/// (CR+LF). Other bytes, including ANSI escapes, pass through.
fn translate_amiga_line_endings(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if i + 1 < input.len() && input[i] == 0x08 && input[i + 1] == b'\n' {
            out.push(b'\r');
            out.push(b'\n');
            i += 2;
        } else {
            out.push(input[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translate_amiga_line_endings_replaces_bs_lf() {
        assert_eq!(translate_amiga_line_endings(b"foo\x08\nbar"), b"foo\r\nbar");
    }

    #[test]
    fn translate_amiga_line_endings_preserves_ansi_escapes() {
        let ansi = b"\x1b[31mRED\x1b[0m\x08\n";
        assert_eq!(
            translate_amiga_line_endings(ansi),
            b"\x1b[31mRED\x1b[0m\r\n"
        );
    }

    #[test]
    fn translate_amiga_line_endings_leaves_other_bytes_alone() {
        assert_eq!(translate_amiga_line_endings(b"hello"), b"hello");
        assert_eq!(translate_amiga_line_endings(b"a\x08b"), b"a\x08b");
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
}
