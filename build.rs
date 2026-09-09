fn main() {
    println!("cargo::rerun-if-changed=assets/branding/icon.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("assets/branding/icon.ico")
        .set("ProductName", env!("CARGO_PKG_NAME"))
        .set("FileDescription", env!("CARGO_PKG_DESCRIPTION"));
    resource
        .compile()
        .expect("failed to compile the Windows executable resources");
}
