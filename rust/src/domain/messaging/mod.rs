//! Messaging domain: mail entities, persistence ports, and the
//! `messaging.allium` rule family.
//!
//! Groups the per-msgbase mail entity ([`mail::Mail`]), the persistence
//! port ([`mail_store::MailStore`]) and the four messaging rules
//! ([`read_mail`], [`scan_mail`], [`post_mail`],
//! [`post_comment_to_sysop`]) so the domain root stays readable as
//! more rule families land. The per-user read pointers
//! ([`read_pointers::ReadPointers`]) live here too — they're consumed
//! exclusively by these rules even though they're attached to
//! `ConferenceMembership` rows on the user.
//!
//! Per the hexagonal layout enforced by `tests/architecture.rs`, this
//! module remains pure domain code and must not depend on
//! [`crate::adapters`] or [`crate::app`].

pub mod mail;
pub mod mail_store;
pub mod post_comment_to_sysop;
pub mod post_mail;
pub mod read_mail;
pub mod read_pointers;
pub mod reply_to_mail;
pub mod scan_mail;
