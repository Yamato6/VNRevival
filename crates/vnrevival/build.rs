//! Embeds the icon and the version information in the Windows executable,
//! and links clang's runtime on macOS.

use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icon/vnrevival.ico");
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("windows") => {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("../../assets/icon/vnrevival.ico")
                .set("ProductName", "VNRevival")
                .set("FileDescription", "VNRevival");
            if let Err(e) = res.compile() {
                panic!("could not embed the Windows resources: {e}");
            }
        }
        Ok("macos") => link_clang_runtime(),
        _ => {}
    }
}

/// SDL, built from source, checks the macOS version at run time
/// (`@available`) when it is built for an older macOS than the SDK's
/// (`MACOSX_DEPLOYMENT_TARGET`). Those checks call
/// `__isPlatformVersionAtLeast` from clang's runtime library, which
/// rustc does not link by itself.
fn link_clang_runtime() {
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
    let Ok(output) = Command::new("clang").arg("--print-resource-dir").output() else {
        println!("cargo:warning=clang not found: clang's runtime is not linked");
        return;
    };
    let dir = String::from_utf8_lossy(&output.stdout);
    let library = Path::new(dir.trim()).join("lib/darwin/libclang_rt.osx.a");
    if library.is_file() {
        println!("cargo:rustc-link-arg={}", library.display());
    } else {
        println!(
            "cargo:warning={} not found: clang's runtime is not linked",
            library.display()
        );
    }
}
