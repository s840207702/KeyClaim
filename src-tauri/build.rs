fn main() {
    #[cfg(target_os = "macos")]
    cc::Build::new()
        .file("native/logitech_hidpp_macos.c")
        .file("native/cursor_macos.m")
        .flag("-Wno-deprecated-declarations")
        .compile("keyclaim_logitech_macos");

    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-lib=framework=IOKit");
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
        println!("cargo:rustc-link-lib=framework=AppKit");
    }

    tauri_build::build()
}
