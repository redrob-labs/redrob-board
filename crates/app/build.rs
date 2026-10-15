//! Windows only: embed the app icon and version info (VERSIONINFO) into `redrob-board.exe`.
//!
//! On every other target this does nothing. A missing resource compiler is a warning, so a
//! cross-compile from macOS or Linux still links, unless `REDROB_BOARD_REQUIRE_WINRES=1` (set
//! by the release packaging) turns it into an error.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/app-icon/redrob-board.ico");
    println!("cargo:rerun-if-env-changed=REDROB_BOARD_REQUIRE_WINRES");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/app-icon/redrob-board.ico")
        .set("ProductName", "Redrob Board")
        .set("FileDescription", "Redrob Board")
        .set("CompanyName", "Redrob")
        .set("LegalCopyright", "Copyright (c) 2026 Janghoon Lee (Redrob) and contributors. MIT OR Apache-2.0.")
        .set("OriginalFilename", "redrob-board.exe")
        .set("InternalName", "redrob-board");
    if let Err(e) = res.compile() {
        if std::env::var_os("REDROB_BOARD_REQUIRE_WINRES").is_some() {
            println!("cargo:warning=embedding Windows resources failed: {e}");
            std::process::exit(1);
        }
        println!("cargo:warning=redrob-board.exe built without icon/version resources: {e}");
    }
}
