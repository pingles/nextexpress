//! The `F` command — NextScan file listings (slice D2).
//!
//! Parity target: the AquaScan v1.0 door experience with NextScan
//! branding (`comparison/evidence-tierD/live-observations.md`;
//! cleanest captures in `comparison/transcripts/ae_tierd_aquascan3.txt`).
//! The shadowed internal `internalCommandF`
//! (`amiexpress/express.e:24877`) is the stock diff record only.

use crate::app::menu_command::FileListArg;
use crate::app::terminal::Terminal;
use crate::domain::session::typed::MenuSession;

impl<T> super::MenuFlow<'_, T>
where
    T: Terminal,
{
    /// Drives the `F` menu command. Stub: the NextScan renderer and
    /// pager land with the next slices of this unit.
    pub(super) async fn handle_file_list(
        &mut self,
        session: &mut MenuSession,
        arg: FileListArg,
    ) -> Result<(), T::Error> {
        let _ = (session, arg);
        Ok(())
    }
}
