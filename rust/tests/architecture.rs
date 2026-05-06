//! Architectural guard for the hexagonal layout.
//!
//! The `domain` layer must not depend on `adapters` or `app`. We
//! enforce this by walking `src/domain/` and rejecting any source
//! file that names the forbidden modules in a `use` path. Phase 0
//! ships empty modules, so the only way this test can fail is by
//! someone introducing a real violation; on a clean tree it passes.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

/// Roots `domain` is forbidden to import from.
const FORBIDDEN_ROOTS: &[&str] = &["adapters", "app"];

/// Walks `dir` recursively and returns every `*.rs` file it contains.
fn rust_sources(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {} failed: {e}", dir.display()));
    for entry in entries {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            out.extend(rust_sources(&path));
        } else if path.extension() == Some(OsStr::new("rs")) {
            out.push(path);
        }
    }
    out
}

/// Returns true when `line` is a `use` statement that names a
/// forbidden sibling module of `domain`.
///
/// We strip whitespace and an optional `pub` qualifier first and then
/// look for the canonical forms — `crate::adapters`, `super::adapters`
/// or a bare top-level `adapters::` — that the Rust module system
/// actually resolves to those siblings. Mentions of the words inside
/// comments, strings or identifiers are deliberately ignored.
fn use_violates(line: &str, forbidden: &str) -> bool {
    let trimmed = line.trim_start();
    let after_pub = trimmed
        .strip_prefix("pub ")
        .map_or(trimmed, str::trim_start);
    let Some(rest) = after_pub.strip_prefix("use ") else {
        return false;
    };
    let target = rest.trim_start();
    target.starts_with(&format!("crate::{forbidden}"))
        || target.starts_with(&format!("super::{forbidden}"))
        || target.starts_with(&format!("{forbidden}::"))
        || target.trim_end_matches([';', ' ']) == forbidden
}

#[test]
fn domain_does_not_depend_on_adapters_or_app() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let domain_dir = Path::new(manifest_dir).join("src").join("domain");

    let mut violations: Vec<String> = Vec::new();
    for path in rust_sources(&domain_dir) {
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {} failed: {e}", path.display()));
        for (idx, line) in content.lines().enumerate() {
            for forbidden in FORBIDDEN_ROOTS {
                if use_violates(line, forbidden) {
                    violations.push(format!(
                        "{}:{} imports `{}` ({})",
                        path.display(),
                        idx + 1,
                        forbidden,
                        line.trim()
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "domain layer must not depend on sibling modules; found:\n{}",
        violations.join("\n")
    );
}

#[test]
fn use_violates_detects_canonical_forms() {
    assert!(use_violates("use crate::adapters;", "adapters"));
    assert!(use_violates("use crate::adapters::Foo;", "adapters"));
    assert!(use_violates("    use super::adapters::Foo;", "adapters"));
    assert!(use_violates("pub use crate::app;", "app"));
    assert!(use_violates("use adapters::Foo;", "adapters"));
}

#[test]
fn use_violates_ignores_unrelated_mentions() {
    assert!(!use_violates("// adapters are great", "adapters"));
    assert!(!use_violates("    let adapters = 3;", "adapters"));
    assert!(!use_violates("use std::collections::HashMap;", "adapters"));
    assert!(!use_violates("use crate::adapter_helpers;", "adapters"));
}
