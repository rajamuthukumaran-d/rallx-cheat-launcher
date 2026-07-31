fn main() {
    slint_build::compile("ui/appwindow.slint").unwrap();

    // Guarded on target_os (not just "Windows-only app" by convention)
    // because build.rs also runs when docs.rs / non-Windows CI builds this
    // crate, where WindowsResource::compile() would fail outright.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/images/logo/logo.ico")
            .compile()
            .unwrap();
    }
}
