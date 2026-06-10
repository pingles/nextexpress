//! File-area domain: entities and ports for the Tier D file subsystem
//! (spec: `files.allium`, `core.allium:FileArea`).
//!
//! Browse-first per `slices/cmds-files-list.md`: the D1+D2 unit ships
//! the listing-visible slice of the `File` lifecycle; transfer-side
//! rules grow in their own slices.

pub mod area;
pub mod file;
pub mod repository;
