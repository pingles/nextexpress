//! Terminal-free menu command use cases.
//!
//! These modules own repository/store resolution and typed session
//! rule invocation for menu commands. `menu_flow` remains responsible
//! for prompts and wire rendering.

pub(crate) mod join;
pub(crate) mod post_mail;
pub(crate) mod read_mail;
pub(crate) mod reply_forward;
pub(crate) mod scan_all_mail;
pub(crate) mod scan_mail;
pub(crate) mod sysop_admin;
