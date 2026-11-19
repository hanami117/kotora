//! Windows only: builds the program icon, the version details (Explorer → Properties → Details)
//! and the manifest (`packaging/windows/kotora.exe.manifest`) into `kotora.exe`. On Linux and
//! macOS this script does nothing, and `winresource` isn't even compiled.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(windows)]
    resources();
}

#[cfg(windows)]
fn resources() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let dir = "../packaging/windows";
    for f in ["kotora.ico", "kotora.exe.manifest"] {
        println!("cargo:rerun-if-changed={dir}/{f}");
    }
    println!("cargo:rerun-if-changed=../BUILD");
    // The fourth number of the file version is the build (the `BUILD` file).
    let build: u64 = std::fs::read_to_string("../BUILD")
        .ok()
        .and_then(|b| b.trim().parse().ok())
        .unwrap_or(1);
    let num = |k: &str| {
        std::env::var(k)
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0)
    };
    let version = (num("CARGO_PKG_VERSION_MAJOR") << 48)
        | (num("CARGO_PKG_VERSION_MINOR") << 32)
        | (num("CARGO_PKG_VERSION_PATCH") << 16)
        | (build & 0xffff);
    let mut res = winresource::WindowsResource::new();
    res.set_icon(&format!("{dir}/kotora.ico"))
        .set_manifest_file(&format!("{dir}/kotora.exe.manifest"))
        .set_version_info(winresource::VersionInfo::FILEVERSION, version)
        .set_version_info(winresource::VersionInfo::PRODUCTVERSION, version)
        .set("ProductName", "Kotora")
        .set("FileDescription", "Kotora")
        .set("CompanyName", "Kotora")
        .set(
            "LegalCopyright",
            "Copyright (C) 2026 Thomas Lonigro. GPL-3.0-or-later",
        )
        .set("OriginalFilename", "kotora.exe")
        .set("InternalName", "kotora");
    // Never fail the build over the resources: a copy without an icon still runs.
    if let Err(e) = res.compile_for("kotora") {
        println!("cargo:warning=no Windows resources in kotora.exe: {e}");
    }
}
