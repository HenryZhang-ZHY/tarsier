fn main() {
    // GPUI loads resource id 1 for the window class icon, so the same embedded
    // icon covers the exe in Explorer, the taskbar and the title bar.
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/icon.ico");
        winresource::WindowsResource::new()
            .set_icon("assets/icon.ico")
            .compile()
            .expect("embed icon resource");
    }
}
