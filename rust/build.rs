//! Build script: captures the current git short SHA into the
//! `NEXTEXPRESS_GIT_SHA` compile-time environment variable so the
//! binary's connect banner can show the source commit it was built
//! from.
//!
//! If `git rev-parse` is unavailable (e.g. building from a release
//! tarball outside a working tree) the SHA falls back to `unknown` so
//! the build still succeeds — the banner just won't pin a commit.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let sha = git_short_sha().unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=NEXTEXPRESS_GIT_SHA={sha}");

    // The Rust crate lives under `rust/`; the repo's `.git` directory
    // is one level up. Re-run the build script when HEAD or the
    // current branch's ref file changes so a fresh commit invalidates
    // the captured SHA.
    let git_dir = PathBuf::from("..").join(".git");
    let head_path = git_dir.join("HEAD");
    if head_path.exists() {
        println!("cargo:rerun-if-changed={}", head_path.display());
        if let Some(ref_path) = current_ref_path(&git_dir, &head_path) {
            if ref_path.exists() {
                println!("cargo:rerun-if-changed={}", ref_path.display());
            }
        }
    }
}

/// Returns the short git SHA for `HEAD`, or `None` if the command
/// fails (no git binary, no repository, detached worktree, etc.).
fn git_short_sha() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if sha.is_empty() {
        None
    } else {
        Some(sha)
    }
}

/// If `HEAD` is a symbolic ref like `ref: refs/heads/main`, returns
/// the path to the underlying ref file so the build script can
/// re-run when that branch moves. Returns `None` for detached HEADs.
fn current_ref_path(git_dir: &Path, head_path: &Path) -> Option<PathBuf> {
    let head = std::fs::read_to_string(head_path).ok()?;
    let ref_rel = head.trim().strip_prefix("ref: ")?;
    Some(git_dir.join(ref_rel))
}
