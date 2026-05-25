fn main() {
    // Link Metal, Foundation and Accelerate frameworks on macOS to enable GPU backend
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-lib=framework=Metal");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=Accelerate");
    }

    #[cfg(not(target_os = "macos"))]
    {
        println!("cargo:rustc-link-lib=vulkan");
    }

    tauri_build::build()
}

