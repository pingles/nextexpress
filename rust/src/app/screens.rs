//! Application port for screen content.
//!
//! The telnet adapter needs bytes for BBS screens, but it should not
//! know whether those bytes come from disk, memory, or another source.

use std::future::Future;
use std::pin::Pin;

/// Future returned by [`ScreenRepository`] methods.
pub type ScreenFuture<'a> = Pin<Box<dyn Future<Output = Vec<u8>> + Send + 'a>>;

/// Port for loading rendered screen bytes.
pub trait ScreenRepository {
    /// Returns the banner shown immediately after telnet negotiation.
    fn banner(&self) -> ScreenFuture<'_>;

    /// Returns the conference menu screen tailored to the supplied
    /// `access_level`. Mirrors the legacy `findSecurityScreen` walk
    /// (`amiexpress/express.e:6246`): the lookup floors `access_level`
    /// to the nearest multiple of five and tries `Menu<N>.txt` for
    /// each multiple from there down to `5`, falling back to the
    /// plain `Menu.txt` and finally to a built-in stub when no asset
    /// is on disk.
    fn default_menu(&self, access_level: u8) -> ScreenFuture<'_>;

    /// Returns the new-user introduction screen
    /// (`SCREEN_NEWUSERPW`, `amiexpress/express.e:30014`). Rendered to
    /// the user when the `user_typed_NEW` branch of
    /// `session.allium:NameTyped` fires (Slice 19). When the
    /// configured asset is missing the adapter returns a built-in
    /// fallback so the registration sub-flow always has something to
    /// show.
    fn new_user_password(&self) -> ScreenFuture<'_>;

    /// Returns the registration-blocked screen
    /// (`SCREEN_NONEWUSERS`, `amiexpress/express.e:30008`). Rendered
    /// when `core/config.allow_new_users = false` causes
    /// `session.allium:RejectDisallowedRegistration` to fire
    /// (Slice 20a). When the asset is missing the adapter returns a
    /// built-in "registration not available" line.
    fn no_new_users(&self) -> ScreenFuture<'_>;
}
