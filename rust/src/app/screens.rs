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

    /// Returns the menu screen for `conference_number`, preferring
    /// per-conference assets at `Conf<NN>/Menu<level>.txt` /
    /// `Conf<NN>/menu.txt` over the system-wide fallback (Slice 31:
    /// "prefer `Conf<n>/menu.txt` over the hard-coded `Conf02/Menu.txt`
    /// used pre-Phase-5"). The security-level walk inside the
    /// per-conference directory mirrors [`Self::default_menu`].
    /// Falls all the way through to [`Self::default_menu`] when no
    /// per-conference asset is on disk.
    fn conference_menu(&self, conference_number: u32, access_level: u8) -> ScreenFuture<'_>;

    /// Returns the `SCREEN_JOINCONF` asset
    /// (`Screens/JoinConf.txt`, `amiexpress/express.e:6588-6590`).
    /// Rendered immediately before the `Conference Number (1-N): `
    /// prompt when the `J` command falls into its interactive flow
    /// (`amiexpress/express.e:25143`, Tier C C2). Returns empty
    /// bytes when the asset is absent — the reference renders
    /// nothing before the prompt on a default install, so the
    /// caller writes the screen only when non-empty.
    ///
    /// `SCREEN_JOIN` and `SCREEN_JOINED` (`Screens/JOIN.txt`,
    /// `Screens/JOINED.txt`) are deliberately *not* part of the
    /// conference-join port: in the legacy source they are
    /// new-user-flow welcome screens displayed by
    /// `processNewUserRegistration` (`amiexpress/express.e:30057`,
    /// `:30125`), not by `joinConf`. The conference-join wire
    /// output (`Conference <n>: <name> Auto-ReJoined` /
    /// `Joining Conference: <name>`) is hardcoded inline at
    /// `amiexpress/express.e:5071-5085` and rendered by the driver
    /// via [`crate::app::wire_text::auto_rejoin_line`] /
    /// [`crate::app::wire_text::explicit_join_line`].
    fn joinconf_screen(&self) -> ScreenFuture<'_>;

    /// Returns the screen shown before the
    /// `Message Base Number (1-N): ` prompt (Tier C C4b), resolved
    /// for `conference_number`: the conference-local
    /// `SCREEN_CONF_JOINMSGBASE` asset (`Conf<NN>/JoinMsgBase.txt`,
    /// `amiexpress/express.e:6591-6593`) first, else the node-level
    /// `SCREEN_JOINMSGBASE` (`Screens/JoinMsgBase.txt`,
    /// `amiexpress/express.e:6594-6596`) — the lookup order of
    /// `internalCommandJM` (`amiexpress/express.e:25221-25222`) and
    /// `internalCommandJ`'s message-base prompt (`:25169-25172`).
    ///
    /// `conference_number` is the *current* conference at prompt
    /// time: the legacy `confScreenDir` is repointed only inside
    /// `joinConf` (`amiexpress/express.e:5052`), so even when `J`
    /// prompts for another conference's bases the screen comes from
    /// the conference the caller is still in.
    ///
    /// Returns empty bytes when neither asset is installed — the
    /// reference renders nothing before the prompt, so the caller
    /// writes the screen only when non-empty.
    fn joinmsgbase_screen(&self, conference_number: u32) -> ScreenFuture<'_>;

    /// Returns the `SCREEN_REALNAMES` asset (Slice 34,
    /// `amiexpress/express.e:28169`). Rendered the first time a join
    /// flips the session's `display_name_type` to
    /// `NameType::RealName`.
    fn realnames_screen(&self) -> ScreenFuture<'_>;

    /// Returns the `SCREEN_INTERNETNAMES` asset (Slice 34,
    /// `amiexpress/express.e:28169`). Rendered the first time a join
    /// flips the session's `display_name_type` to
    /// `NameType::InternetName`.
    fn internetnames_screen(&self) -> ScreenFuture<'_>;

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

    /// Returns the `SCREEN_MAILSCAN` asset (Slice 41,
    /// `amiexpress/axenums.e:19`). Rendered immediately before the
    /// scan-summary line whenever an auto-scan-on-join surfaces at
    /// least one unread message — gives the sysop a hook for a
    /// "you've got new mail" splash.
    fn mailscan_screen(&self) -> ScreenFuture<'_>;

    /// Returns the BBS help screen (`<bbs-loc>/BBSHelp.txt`,
    /// Tier A quickwin A5, `amiexpress/express.e:25079-25085`).
    ///
    /// Used by the `H` menu command. Returns empty bytes when the
    /// asset is absent so the caller can fall back to the legacy
    /// `Sorry Help is unavailable at this time.` line
    /// ([`crate::app::wire_text::HELP_UNAVAILABLE_LINE`]) instead of
    /// silently rendering a built-in stub.
    fn bbs_help_screen(&self) -> ScreenFuture<'_>;

    /// Returns the topic-help screen for `topic`, read from
    /// `<bbs-loc>/help/<topic>.txt` (Tier A quickwin A10,
    /// `amiexpress/express.e:25089-25110`). When no file matches the
    /// full topic, the lookup strips trailing characters one at a time
    /// and retries (`^FILES` → `FILE` → `FIL` → …), mirroring the
    /// legacy `internalCommandUpHat` truncate-and-retry loop.
    ///
    /// Used by the `^` menu command. Returns empty bytes when no prefix
    /// of `topic` (including the empty topic) matches a screen, so the
    /// caller can render the result unconditionally and skip silently.
    fn topic_help(&self, topic: &str) -> ScreenFuture<'_>;

    /// Returns the `SCREEN_LOGOFF` asset
    /// (`Screens/LOGOFF.txt`, `amiexpress/express.e:6554`, displayed at
    /// `:8187`). Rendered on a normal user-requested logoff
    /// (`G` menu command), immediately before the
    /// [`crate::app::wire_text::GOODBYE_LINE`].
    ///
    /// The legacy gates rendering on `logonType != LOGON_TYPE_SYSOP`
    /// and `ftpConn = FALSE`; `NextExpress` has neither sysop direct
    /// logon nor an FTP channel yet, so every normal logoff gets the
    /// screen. Idle timeout / account lock / carrier loss exits emit
    /// their dedicated goodbye lines (`IDLE_TIMEOUT_LINE`,
    /// `ACCOUNT_LOCKED_LINE`, etc.) and skip the screen — matching
    /// the legacy which only invokes `displayScreen(SCREEN_LOGOFF)`
    /// from the clean-logoff path.
    ///
    /// Returns empty bytes when the asset is absent so the caller can
    /// write the result unconditionally and skip silently.
    fn logoff_screen(&self) -> ScreenFuture<'_>;
}
