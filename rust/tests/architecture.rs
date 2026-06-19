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
/// Runtime / adapter crates that must not appear in domain code.
const FORBIDDEN_INFRASTRUCTURE_REFERENCES: &[&str] = &[
    "tokio::",
    "serde_json::",
    "toml::",
    "std::fs::",
    "std::io::",
    "std::net::",
];

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

fn source_before_line_comment(line: &str) -> &str {
    line.split_once("//").map_or(line, |(code, _comment)| code)
}

fn is_path_boundary_before(ch: char) -> bool {
    !ch.is_ascii_alphanumeric() && ch != '_' && ch != ':'
}

fn is_path_boundary_after(ch: char) -> bool {
    !ch.is_ascii_alphanumeric() && ch != '_'
}

fn contains_path_prefix(code: &str, prefix: &str) -> bool {
    code.match_indices(prefix).any(|(idx, _)| {
        let before_ok = code[..idx]
            .chars()
            .next_back()
            .is_none_or(is_path_boundary_before);
        let after = idx + prefix.len();
        let after_ok = prefix.ends_with("::")
            || code[after..]
                .chars()
                .next()
                .is_none_or(is_path_boundary_after);
        before_ok && after_ok
    })
}

fn grouped_use_contains(target: &str, root: &str, forbidden: &str) -> bool {
    let Some(rest) = target.strip_prefix(root) else {
        return false;
    };
    let Some(group) = rest.trim_start().strip_prefix('{') else {
        return false;
    };
    let group = group.split('}').next().unwrap_or(group);
    group.split(',').any(|item| {
        let item = item.trim_start();
        item == forbidden
            || item.starts_with(&format!("{forbidden}::"))
            || item.starts_with(&format!("{forbidden} as "))
    })
}

/// Returns true when a non-comment source line names a forbidden
/// sibling module. Unlike [`use_violates`], this also catches
/// fully-qualified references such as `crate::adapters::Foo::new()`,
/// which couple production code to another layer without an import.
fn references_forbidden_module(line: &str, forbidden: &str) -> bool {
    let code = source_before_line_comment(line);
    if code.trim().is_empty() {
        return false;
    }
    if contains_path_prefix(code, &format!("crate::{forbidden}"))
        || contains_path_prefix(code, &format!("super::{forbidden}"))
        || contains_path_prefix(code, &format!("{forbidden}::"))
    {
        return true;
    }

    let trimmed = code.trim_start();
    let after_pub = trimmed
        .strip_prefix("pub ")
        .map_or(trimmed, str::trim_start);
    let Some(rest) = after_pub.strip_prefix("use ") else {
        return false;
    };
    let target = rest.trim_start();
    grouped_use_contains(target, "crate::", forbidden)
        || grouped_use_contains(target, "super::", forbidden)
}

/// Returns true when a non-comment source line names an
/// infrastructure-only crate or module. This intentionally checks
/// fully-qualified references as well as `use` lines, because a domain
/// error such as `source: serde_json::Error` couples the domain to an
/// adapter detail without needing an import.
fn references_infrastructure(line: &str, forbidden: &str) -> bool {
    source_before_line_comment(line).contains(forbidden)
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
                if references_forbidden_module(line, forbidden) {
                    violations.push(format!(
                        "{}:{} references `{}` ({})",
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
fn domain_does_not_depend_on_runtime_or_adapter_crates() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let domain_dir = Path::new(manifest_dir).join("src").join("domain");

    let mut violations: Vec<String> = Vec::new();
    for path in rust_sources(&domain_dir) {
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {} failed: {e}", path.display()));
        for (idx, line) in content.lines().enumerate() {
            for forbidden in FORBIDDEN_INFRASTRUCTURE_REFERENCES {
                if references_infrastructure(line, forbidden) {
                    violations.push(format!(
                        "{}:{} references `{}` ({})",
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
        "domain layer must stay free of runtime/adapter crates; found:\n{}",
        violations.join("\n")
    );
}

/// Returns `content` with every top-level `#[cfg(test)] mod <name> {
/// … }` block elided. Block boundaries are tracked by counting `{` /
/// `}` from the opening brace of the `mod` declaration; nested braces
/// inside the test module (function bodies, struct literals, etc.) are
/// followed correctly.
///
/// The guard for `app/` and `bootstrap` policy only inspects production
/// code, since unit tests legitimately reach for adapter test doubles
/// (`InMemoryUserRepository`, `Pbkdf2PasswordHasher`, etc.) and the
/// hexagonal boundary is a production-code invariant. Reusing the same
/// helper for both the use-import and infrastructure-reference checks
/// keeps the policy consistent.
fn strip_cfg_test_modules(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut in_test_mod = false;
    let mut depth: i32 = 0;
    let mut pending_test_mod = false;
    for line in content.lines() {
        if in_test_mod {
            for ch in line.chars() {
                if ch == '{' {
                    depth += 1;
                } else if ch == '}' {
                    depth -= 1;
                    if depth == 0 {
                        in_test_mod = false;
                        break;
                    }
                }
            }
            out.push('\n');
            continue;
        }
        let trimmed = line.trim_start();
        if pending_test_mod {
            if let Some(rest) = trimmed
                .strip_prefix("pub ")
                .map_or(Some(trimmed), |r| Some(r.trim_start()))
                .and_then(|t| t.strip_prefix("mod "))
            {
                if let Some(open) = rest.find('{') {
                    in_test_mod = true;
                    depth = 0;
                    for ch in rest[open..].chars() {
                        if ch == '{' {
                            depth += 1;
                        } else if ch == '}' {
                            depth -= 1;
                        }
                    }
                    if depth == 0 {
                        in_test_mod = false;
                    }
                    pending_test_mod = false;
                    out.push('\n');
                    continue;
                }
            }
            // Not a `mod` declaration after the attribute — treat the
            // attribute line as ordinary code, including the line we are
            // currently inspecting.
            pending_test_mod = false;
        }
        if trimmed.starts_with("#[cfg(test)]") {
            pending_test_mod = true;
            out.push('\n');
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// A sibling test module (`#[cfg(test)] mod tests;` -> `tests.rs`, or a
/// `tests/` submodule directory) is test-only code, exactly like an
/// inline `#[cfg(test)] mod tests { ... }`. The walker strips inline test
/// modules via [`strip_cfg_test_modules`]; sibling test files have no
/// in-file `#[cfg(test)]` marker (the attribute is on the parent's `mod`
/// declaration), so they are excluded here by path. This mirrors how
/// `rust-lang/rust`'s own `tidy` tool classifies test code — by the
/// `tests.rs` / `tests` name, not by attribute.
fn is_sibling_test_module(path: &Path) -> bool {
    path.file_name().and_then(OsStr::to_str) == Some("tests.rs")
        || path.components().any(|c| c.as_os_str() == "tests")
}

#[test]
fn app_does_not_depend_on_adapters_in_production_code() {
    // The hexagonal boundary lets `app/` depend only on `domain/` and
    // its own ports. Adapter construction is the bootstrap layer's job.
    // Test code legitimately needs adapter test doubles; the guard
    // therefore strips `#[cfg(test)] mod …` blocks and skips sibling
    // `tests.rs` test modules before scanning.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let app_dir = Path::new(manifest_dir).join("src").join("app");

    let mut violations: Vec<String> = Vec::new();
    for path in rust_sources(&app_dir) {
        if is_sibling_test_module(&path) {
            continue;
        }
        let raw = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {} failed: {e}", path.display()));
        let content = strip_cfg_test_modules(&raw);
        for (idx, line) in content.lines().enumerate() {
            if references_forbidden_module(line, "adapters") {
                violations.push(format!(
                    "{}:{} references `adapters` ({})",
                    path.display(),
                    idx + 1,
                    line.trim()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "app layer must not import adapters in production code; found:\n{}",
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

#[test]
fn forbidden_module_reference_detects_fully_qualified_code() {
    assert!(references_forbidden_module(
        "let repo = crate::adapters::SqliteUserRepository::open(path);",
        "adapters"
    ));
    assert!(references_forbidden_module(
        "type Flow = crate::app::session_flow::NameTypedTransition;",
        "app"
    ));
    assert!(references_forbidden_module(
        "let _ = adapters::InMemoryUserRepository::default();",
        "adapters"
    ));
}

#[test]
fn forbidden_module_reference_detects_grouped_imports() {
    assert!(references_forbidden_module(
        "use crate::{adapters::Foo, domain::Bar};",
        "adapters"
    ));
    assert!(references_forbidden_module(
        "use crate::{adapters, domain};",
        "adapters"
    ));
    assert!(references_forbidden_module(
        "pub use super::{app as app_layer, domain};",
        "app"
    ));
}

#[test]
fn forbidden_module_reference_ignores_comments_and_identifiers() {
    assert!(!references_forbidden_module(
        "//! see crate::adapters::SqliteUserRepository",
        "adapters"
    ));
    assert!(!references_forbidden_module(
        "let adapters = 3; // crate::adapters::Foo",
        "adapters"
    ));
    assert!(!references_forbidden_module(
        "let helper = crate::adapter_helpers::Fixture;",
        "adapters"
    ));
    assert!(!references_forbidden_module(
        "let helper = mycrate::adapters::Fixture;",
        "adapters"
    ));
}

#[test]
fn is_sibling_test_module_classifies_test_files() {
    assert!(is_sibling_test_module(Path::new(
        "src/app/menu_flow/file_list/tests.rs"
    )));
    assert!(is_sibling_test_module(Path::new(
        "src/app/foo/tests/mod.rs"
    )));
    assert!(!is_sibling_test_module(Path::new(
        "src/app/menu_flow/file_list/mod.rs"
    )));
    assert!(!is_sibling_test_module(Path::new("src/app/login_flow.rs")));
}

#[test]
fn references_infrastructure_ignores_comments_but_catches_code() {
    assert!(!references_infrastructure(
        "//! adapter uses serde_json::Error",
        "serde_json::"
    ));
    assert!(references_infrastructure(
        "source: serde_json::Error,",
        "serde_json::"
    ));
}

#[test]
fn strip_cfg_test_modules_removes_test_blocks_only() {
    let input = "use crate::adapters::Foo;\n\
                 fn prod() {}\n\
                 #[cfg(test)]\n\
                 mod tests {\n    use crate::adapters::Bar;\n    fn t() { let _ = 1; }\n}\n\
                 fn more_prod() {}\n";
    let stripped = strip_cfg_test_modules(input);
    assert!(
        stripped.contains("use crate::adapters::Foo;"),
        "production import retained: {stripped:?}"
    );
    assert!(
        stripped.contains("fn more_prod"),
        "code after test mod retained: {stripped:?}"
    );
    assert!(
        !stripped.contains("use crate::adapters::Bar;"),
        "test-mod import stripped: {stripped:?}"
    );
}

#[test]
fn strip_cfg_test_modules_keeps_cfg_test_attribute_on_items() {
    // A bare `#[cfg(test)]` on a function (not a `mod`) must not
    // accidentally swallow the function. The boundary is `mod …`.
    let input = "#[cfg(test)]\n\
                 fn helper() { let _ = 1; }\n\
                 fn prod() {}\n";
    let stripped = strip_cfg_test_modules(input);
    assert!(
        stripped.contains("fn helper"),
        "test-only function preserved: {stripped:?}"
    );
    assert!(
        stripped.contains("fn prod"),
        "prod function preserved: {stripped:?}"
    );
}
