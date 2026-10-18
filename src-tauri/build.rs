fn main() {
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rerun-if-changed=native/logitech_hidpp_macos.m");
        println!("cargo:rerun-if-changed=native/input_macos.c");
        cc::Build::new()
            .file("native/logitech_hidpp_macos.m")
            .file("native/input_macos.c")
            .flag("-Wno-deprecated-declarations")
            .compile("keyclaim_logitech_macos");
    }

    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-lib=framework=IOKit");
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
        println!("cargo:rustc-link-lib=framework=ApplicationServices");
        println!("cargo:rustc-link-lib=objc");
    }

    #[cfg(target_os = "windows")]
    {
        println!("cargo:rerun-if-changed=native/input_windows.c");
        cc::Build::new()
            .file("native/input_windows.c")
            .compile("keyclaim_input_windows");
        println!("cargo:rustc-link-lib=user32");
    }

    tauri_build::build()
}
