//! Shared `#[cfg(test)]` fixtures for the `file_list` test modules.
//!
//! These helpers compose expected wire bytes from the subtree-private
//! `wire` renderers, so they live under `file_list` (visibility is
//! subtree-closed) rather than in `menu_flow/test_support.rs` with the
//! wire-agnostic fixtures.

use std::time::SystemTime;

use crate::adapters::in_memory_file_repository::InMemoryFileRepository;
use crate::app::menu_command::FileListArg;
use crate::app::menu_flow::test_support::{menu_session, services_with, CaptureTerminal};
use crate::app::services::AppServices;
use crate::domain::files::area::FileAreaRef;
use crate::domain::files::flagged::FlaggedFiles;

use super::wire;

/// Drives the `F` handler against a scripted terminal. The fake
/// terminal does not echo, so `terminal.output` is the pure
/// server-generated wire — the parity surface.
pub(super) async fn run_file_list(
    services: &AppServices,
    terminal: &mut CaptureTerminal,
    arg: FileListArg,
) {
    let mut session = menu_session();
    let mut flow = crate::app::menu_flow::MenuFlow { terminal, services };
    flow.handle_file_list(&mut session, arg)
        .await
        .expect("file list");
}

/// `\x1b[0m\r\n` + listing banner + blank — §1.1's entry preamble
/// for every argument form (`ae_tierd_aquascan3.txt:163/217`).
pub(super) fn listing_preamble() -> Vec<u8> {
    let mut bytes = b"\x1b[0m\r\n".to_vec();
    bytes.extend_from_slice(wire::LISTING_BANNER);
    bytes.extend_from_slice(b"\r\n\r\n");
    bytes
}

/// Every `\r\n`-terminated line the `F 1` span emits before its
/// body pauses: reset-blank, banner, blank, scan header, blank,
/// then the assembled dir-1 lines.
pub(super) fn f_1_emitted_lines(services: &AppServices) -> Vec<Vec<u8>> {
    let mut lines: Vec<Vec<u8>> = vec![
        b"\x1b[0m".to_vec(),
        wire::LISTING_BANNER.to_vec(),
        Vec::new(),
        b"Scanning dir 1 from top... Ok!".to_vec(),
        Vec::new(),
    ];
    lines.extend(
        wire::assemble_dir_lines(
            &services
                .file_repo
                .find_in_area(FileAreaRef::new(1, 1))
                .expect("files"),
            1,
            &FlaggedFiles::default(),
            false,
        )
        .into_iter()
        .map(|line| line.bytes),
    );
    lines
}

/// The assembled listing lines of `(conference 1, area)` under the
/// default (empty) flag set.
pub(super) fn area_lines(services: &AppServices, area: u32) -> Vec<Vec<u8>> {
    wire::assemble_dir_lines(
        &services
            .file_repo
            .find_in_area(FileAreaRef::new(1, area))
            .expect("files"),
        1,
        &FlaggedFiles::default(),
        false,
    )
    .into_iter()
    .map(|line| line.bytes)
    .collect()
}

/// A small two-area catalogue (1 file each) for choreography
/// tests that must not hit the 29-line page boundary.
pub(super) fn services_with_two_small_areas() -> AppServices {
    use crate::domain::bytes::Bytes;
    use crate::domain::files::area::FileArea;
    use crate::domain::files::file::{File, FileStatus};
    let file = |name: &str| {
        File::new(
            name.to_string(),
            Bytes::new(1_000),
            FileStatus::Available,
            Some(b'P'),
            format!("{name} description"),
            SystemTime::from(time::macros::datetime!(2026-06-01 12:00 UTC)),
        )
    };
    services_with(InMemoryFileRepository::new(
        vec![
            FileArea::new(1, 1, "Main".to_string()),
            FileArea::new(1, 2, "Uploads".to_string()),
        ],
        vec![(1, 1, file("FIRST.LHA")), (1, 2, file("SECOND.LHA"))],
    ))
}
