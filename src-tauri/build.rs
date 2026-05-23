fn main() {
    // Link Metal, Foundation and Accelerate frameworks on macOS to enable GPU backend
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-lib=framework=Metal");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=Accelerate");
    }

    // CUDA support is enabled via environment variables (see README or run with CUDA_HOME=/opt/cuda)
    tauri_build::build()
}

