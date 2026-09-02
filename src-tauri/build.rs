fn main() {
    #[cfg(target_os = "macos")]
    cc::Build::new()
        .file("native/logitech_hidpp_macos.c")
        .flag("-Wno-deprecated-declarations")
        .compile("keyclaim_logitech_macos");

    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-lib=framework=IOKit");
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
    }

    tauri_build::build()
}
