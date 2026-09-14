//! Sets the version string the CLI prints. A build from a git checkout
//! gets `git describe --tags --always --dirty`, so a commit after the last
//! release shows as `0.1.6-16-g7a09b62`. A build without git or without a
//! `.git` folder, such as the Nix package or a release tarball, gets the
//! Cargo version.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let version =
        git_describe(&manifest_dir).unwrap_or_else(|| env::var("CARGO_PKG_VERSION").unwrap());
    println!("cargo:rustc-env=EPUBSYNC_VERSION={version}");
}

/// Runs `git describe` in the crate folder and strips the `v` that the tags
/// carry. Returns None when git is missing or the folder is not a checkout.
fn git_describe(dir: &Path) -> Option<String> {
    let git_dir = git(dir, &["rev-parse", "--git-dir"])?;
    let git_dir = dir.join(git_dir);
    // A new commit or a checkout moves HEAD or the ref it points at.
    println!("cargo:rerun-if-changed={}", git_dir.join("HEAD").display());
    if let Some(head) = git(dir, &["symbolic-ref", "-q", "HEAD"]) {
        println!("cargo:rerun-if-changed={}", git_dir.join(head).display());
    }
    // A new tag lands in refs/tags or, after `git gc`, in packed-refs.
    println!(
        "cargo:rerun-if-changed={}",
        git_dir.join("refs/tags").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        git_dir.join("packed-refs").display()
    );

    let described = git(dir, &["describe", "--tags", "--always", "--dirty"])?;
    Some(
        described
            .strip_prefix('v')
            .unwrap_or(&described)
            .to_string(),
    )
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}
