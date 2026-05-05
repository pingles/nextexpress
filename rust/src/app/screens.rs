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

    /// Returns the default conference menu.
    fn default_menu(&self) -> ScreenFuture<'_>;
}
