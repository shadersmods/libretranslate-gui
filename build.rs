fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winres::WindowsResource::new()
            .set_icon("assets/icon.ico")
            .set("ProductName", "LibreTranslate Desktop")
            .set("FileDescription", "LibreTranslate Desktop")
            .compile()
            .expect("Could not embed the Windows application icon");
    }
}
