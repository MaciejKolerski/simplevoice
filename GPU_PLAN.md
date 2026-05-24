# GPU Acceleration Plan (Windows & Linux)

This document outlines the implementation plan to enable GPU acceleration for local transcription models on **Windows** and **Linux** within `simplevoice`.

Currently, macOS uses native Metal acceleration for Whisper. We will extend support to Windows and Linux using **Vulkan** (for Whisper models) and target-specific ONNX Runtime providers (**DirectML** for Windows, **CUDA** for Linux) for Parakeet models.

---

## 1. Technical Strategy

### A. Whisper (GGML `.bin` models via `whisper-rs`)
*   **API Selection**: We will use the **Vulkan** backend (enabled via the `vulkan` feature flag in `whisper-rs`). Vulkan works across NVIDIA, AMD, and Intel GPUs. This avoids requiring users to install massive CUDA Toolkit packages.
*   **CPU Fallback**: If GPU initialization fails (e.g. outdated graphics drivers or unsupported hardware), the engine will catch the error, log it, and automatically initialize on CPU so the application doesn't hang or crash.

### B. Parakeet (ONNX `.onnx` models via `ort` / ONNX Runtime)
*   **Windows**: Compile `ort` with the **DirectML** feature (`directml`). This works out-of-the-box on any DirectX 12 compatible GPU (NVIDIA, AMD, Intel).
*   **Linux**: Compile `ort` with the **CUDA** feature (`cuda`) for NVIDIA GPUs.
*   **Automatic Fallback**: ONNX Runtime has built-in mechanisms to automatically fall back to CPU if the selected GPU execution provider is unavailable.

### C. Sherpa-ONNX (Moonshine / Canary models)
*   `sherpa-onnx` downloads prebuilt CPU-only static libraries by default. Custom GPU builds require packaging dynamic libraries (`.dll` / `.so`) exceeding 200MB and exact CUDA runtime alignment on Linux.
*   Therefore, Moonshine/Canary models will remain on CPU (highly multithreaded and optimized) for maximum stability.

---

## 2. Code Changes

### Step 1: Dependencies Setup (`src-tauri/Cargo.toml`)
We will introduce target-specific dependencies depending on the compilation target:

```toml
[dependencies]
# Base dependencies without default GPU features
whisper-rs = { version = "0.16.0", default-features = false }
ort = { version = "2.0.0-rc.9", features = ["ndarray"] }

# Windows and Linux GPU configuration (Vulkan for Whisper)
[target.'cfg(any(target_os = "linux", target_os = "windows"))'.dependencies]
whisper-rs = { version = "0.16.0", default-features = false, features = ["vulkan"] }

# DirectML for Windows in ONNX Runtime
[target.'cfg(target_os = "windows")'.dependencies]
ort = { version = "2.0.0-rc.9", features = ["ndarray", "directml"] }

# CUDA for Linux in ONNX Runtime
[target.'cfg(target_os = "linux")'.dependencies]
ort = { version = "2.0.0-rc.9", features = ["ndarray", "cuda"] }
```

### Step 2: Whisper Engine Update (`src-tauri/src/stt/mod.rs`)
Update `WhisperEngine::initialize` to try GPU context creation first, falling back to CPU if it fails:

```rust
impl EngineAdapter for WhisperEngine {
    fn initialize(&mut self, model_path: &str) -> Result<(), String> {
        // Attempt 1: Try GPU (Metal on macOS, Vulkan on Win/Linux)
        let mut params = WhisperContextParameters::default();
        params.use_gpu = true; 
        params.flash_attn = cfg!(target_os = "macos");

        match WhisperContext::new_with_params(model_path, params) {
            Ok(ctx) => {
                if let Ok(whisper_state) = ctx.create_state() {
                    self.context = Some(ctx);
                    self.state = Some(Mutex::new(whisper_state));
                    println!("[WhisperEngine] Initialized successfully using GPU.");
                    return Ok(());
                }
            }
            Err(e) => {
                println!("[WhisperEngine] Failed to initialize GPU: {}. Falling back to CPU...", e);
            }
        }

        // Attempt 2: Fallback to CPU
        let mut params_cpu = WhisperContextParameters::default();
        params_cpu.use_gpu = false;
        params_cpu.flash_attn = false;

        let ctx = WhisperContext::new_with_params(model_path, params_cpu)
            .map_err(|e| format!("Failed to initialize Whisper context (CPU fallback): {}", e))?;

        let whisper_state = ctx
            .create_state()
            .map_err(|e| format!("Failed to create Whisper state: {}", e))?;

        self.context = Some(ctx);
        self.state = Some(Mutex::new(whisper_state));
        println!("[WhisperEngine] Initialized successfully using CPU.");
        Ok(())
    }
}
```

### Step 3: Parakeet Engine Update (`src-tauri/src/stt/parakeet.rs`)
Register DirectML/CUDA execution providers during session builder initialization:

```rust
impl super::EngineAdapter for ParakeetEngine {
    fn initialize(&mut self, model_path: &str) -> Result<(), String> {
        let mut builder = Session::builder()
            .map_err(|e| format!("Failed to create ONNX session builder: {}", e))?;

        // Windows: DirectML Execution Provider
        #[cfg(target_os = "windows")]
        {
            builder = builder
                .with_execution_providers([
                    ort::execution_providers::DirectMLExecutionProvider::default().build(),
                ])
                .map_err(|e| format!("Failed to register DirectML provider: {}", e))?;
            println!("[ParakeetEngine] Registered DirectML provider (will fallback to CPU if unavailable).");
        }

        // Linux: CUDA Execution Provider
        #[cfg(target_os = "linux")]
        {
            builder = builder
                .with_execution_providers([
                    ort::execution_providers::CUDAExecutionProvider::default().build(),
                ])
                .map_err(|e| format!("Failed to register CUDA provider: {}", e))?;
            println!("[ParakeetEngine] Registered CUDA provider (will fallback to CPU if unavailable).");
        }

        let session = builder
            .commit_from_file(model_path)
            .map_err(|e| format!("Failed to load ONNX model from {}: {}", model_path, e))?;

        self.session = Some(Mutex::new(session));
        Ok(())
    }
}
```

---

## 3. System Requirements

### A. Windows Environment
*   **Compile-time**: **Vulkan SDK** (LunarG) must be installed so headers can be resolved during C++ compilation in `whisper-rs`.
*   **Runtime**: DirectX 12 compatible GPU drivers (for DirectML) and Vulkan support.

### B. Linux Environment
*   **Compile-time**: Vulkan headers package (e.g., `vulkan-headers` / `libvulkan-dev`).
*   **Runtime**:
    *   *For Whisper (Vulkan)*: Graphics driver with Vulkan support (Mesa for AMD/Intel, official driver for NVIDIA) and Vulkan loader (`libvulkan.so.1`).
    *   *For Parakeet (CUDA)*: NVIDIA GPU, official NVIDIA proprietary drivers, and CUDA Toolkit runtime installed.

---

## 4. Verification

1. Check Rust compilation: `cargo check --manifest-path src-tauri/Cargo.toml`.
2. Start the dev workflow: `pnpm tauri dev`.
3. Load a local model (GGML `.bin` for Whisper or `.onnx` for Parakeet) in the Models tab.
4. Verify the logs in terminal:
   *   `[WhisperEngine] Initialized successfully using GPU.`
   *   `[ParakeetEngine] Registered DirectML/CUDA provider.`
5. Monitor GPU usage via System Task Manager (Windows) or `nvidia-smi` (Linux NVIDIA) during transcription.
