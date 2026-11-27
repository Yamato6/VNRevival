//! Embeds the icon and the version information in the Windows executable.

fn main() {
    println!("cargo:rerun-if-changed=../../assets/icon/vnrevival.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icon/vnrevival.ico")
            .set("ProductName", "VNRevival")
            .set("FileDescription", "VNRevival");
        if let Err(e) = res.compile() {
            panic!("could not embed the Windows resources: {e}");
        }
    }
}
