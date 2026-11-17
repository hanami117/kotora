//! Records where a dev build comes from, for the version shown in About and `--version`.
//!
//! It emits `KOTORA_GIT` = `branch<TAB>short hash<TAB>dirty (0/1)<TAB>at release tag (0/1)`, or an
//! empty string when git facts are unavailable (a source tarball has no `.git`, git may be missing, or
//! the git top level isn't Kotora's workspace root, as when a tarball is unpacked inside another checkout).
//! The rule that turns these into text is `git_suffix` in `src/lib.rs`. This script never fails the
//! build.

use std::path::Path;
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// True when `top` (git's top level) is the workspace root, i.e. the checkout is Kotora's own.
/// A source tarball unpacked inside some other checkout (an AUR helper's clone) is not.
fn is_workspace_root(top: &Path, root: &Path) -> bool {
    match (top.canonicalize(), root.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn own_repo() -> bool {
    let Some(root) = std::env::var_os("CARGO_MANIFEST_DIR").map(|d| Path::new(&d).join("..")) else { return false };
    git(&["rev-parse", "--show-toplevel"]).is_some_and(|top| is_workspace_root(Path::new(&top), &root))
}

fn facts() -> Option<String> {
    let hash = git(&["rev-parse", "--short=7", "HEAD"]).filter(|h| !h.is_empty())?;
    let branch = git(&["symbolic-ref", "--short", "-q", "HEAD"]).filter(|b| !b.is_empty()).unwrap_or_else(|| "detached".into());
    let dirty = git(&["status", "--porcelain"]).is_none_or(|s| !s.is_empty());

    // The release tag for this version and build: vX.Y.Z for build 1, vX.Y.Z-N after.
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let build: u32 = std::fs::read_to_string("../BUILD").ok().and_then(|s| s.trim().parse().ok()).unwrap_or(1);
    let tag = if build == 1 { format!("v{version}") } else { format!("v{version}-{build}") };
    let at_tag = git(&["tag", "--points-at", "HEAD"]).is_some_and(|t| t.lines().any(|l| l == tag));

    Some(format!("{branch}\t{hash}\t{}\t{}", dirty as u8, at_tag as u8))
}

/// Windows only: the icon, the version details and the manifest of `kot.exe` (as `app/build.rs`
/// does for `kotora.exe`). Never fails the build.
#[cfg(windows)]
fn resources() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let dir = "../packaging/windows";
    for f in ["kotora.ico", "kot.exe.manifest"] {
        println!("cargo:rerun-if-changed={dir}/{f}");
    }
    let build: u64 = std::fs::read_to_string("../BUILD").ok().and_then(|b| b.trim().parse().ok()).unwrap_or(1);
    let num = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    let version = (num("CARGO_PKG_VERSION_MAJOR") << 48) | (num("CARGO_PKG_VERSION_MINOR") << 32) | (num("CARGO_PKG_VERSION_PATCH") << 16) | (build & 0xffff);
    let mut res = winresource::WindowsResource::new();
    res.set_icon(&format!("{dir}/kotora.ico"))
        .set_manifest_file(&format!("{dir}/kot.exe.manifest"))
        .set_version_info(winresource::VersionInfo::FILEVERSION, version)
        .set_version_info(winresource::VersionInfo::PRODUCTVERSION, version)
        .set("ProductName", "Kotora")
        .set("FileDescription", "kot: the command line for Kotora")
        .set("CompanyName", "Kotora")
        .set("LegalCopyright", "GPL-3.0-or-later")
        .set("OriginalFilename", "kot.exe")
        .set("InternalName", "kot");
    if let Err(e) = res.compile_for("kot") {
        println!("cargo:warning=no Windows resources in kot.exe: {e}");
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../BUILD");
    #[cfg(windows)]
    resources();
    if !own_repo() {
        println!("cargo:rustc-env=KOTORA_GIT=");
        return;
    }
    // HEAD moves on checkout and commit (a commit rewrites the branch ref); the index changes on `git add`.
    for p in ["HEAD", "index", "packed-refs"] {
        if let Some(path) = git(&["rev-parse", "--git-path", p]).filter(|s| Path::new(s).exists()) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    if let Some(r) = git(&["symbolic-ref", "-q", "HEAD"]).and_then(|r| git(&["rev-parse", "--git-path", &r])) {
        println!("cargo:rerun-if-changed={r}");
    }
    println!("cargo:rerun-if-changed=../.git/refs/tags");
    println!("cargo:rustc-env=KOTORA_GIT={}", facts().unwrap_or_default());
}
