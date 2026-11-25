//! Builds the app's on-device AI module (`native/`, C++: llama.cpp + whisper.cpp) and links it, then Tauri's build.
//!
//! - macOS: built whenever `tools/local-ai/setup.sh` has put llama.cpp in `native/vendor/` (otherwise a warning,
//!   and the app builds without on-device AI).
//! - Windows / Linux: only with `--features local-ai` (needs CMake). iOS: never.
//! - `PHYSIOPULSE_NO_LOCAL_AI=1` skips it everywhere.
//! When built, the code sees `cfg(local_ai)` (src/local_ai.rs).

use std::path::PathBuf;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(local_ai)");
    native();
    tauri_build::build()
}

fn native() {
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let feature = std::env::var_os("CARGO_FEATURE_LOCAL_AI").is_some();
    println!("cargo:rerun-if-env-changed=PHYSIOPULSE_NO_LOCAL_AI");
    if matches!(os.as_str(), "ios" | "android") || std::env::var_os("PHYSIOPULSE_NO_LOCAL_AI").is_some() || !(os == "macos" || feature) {
        return;
    }
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("native");
    for p in ["src", "include", "CMakeLists.txt", "vendor/llama.cpp/CMakeLists.txt", "vendor/whisper.cpp/CMakeLists.txt"] {
        println!("cargo:rerun-if-changed={}", dir.join(p).display());
    }
    if !dir.join("vendor/llama.cpp/CMakeLists.txt").exists() {
        let msg = "on-device AI not built: run tools/local-ai/setup.sh first";
        if feature {
            panic!("{msg}");
        }
        println!("cargo:warning={msg}");
        return;
    }

    let out = cmake::Config::new(&dir).profile("Release").build();
    let lib = out.join("lib");
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-lib=dylib=physiopulse_ai");
    println!("cargo:rustc-cfg=local_ai");

    // Where the app finds the libraries: in the bundle (Contents/Frameworks), else where they were built (dev).
    if os == "macos" {
        println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
    }
    if os != "windows" {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
    }

    // A fixed place for the bundler (tauri.local-ai.conf.json → bundle.macOS.frameworks).
    let dist = dir.join("dist");
    let _ = std::fs::create_dir_all(&dist);
    if let Ok(entries) = std::fs::read_dir(&lib) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.ends_with(".dylib") || name.ends_with(".so") || name.ends_with(".dll") {
                let _ = std::fs::copy(e.path(), dist.join(&name));
            }
        }
    }
    if os == "windows" {
        // Windows finds DLLs next to the .exe.
        if let Some(target) = out.ancestors().nth(3) {
            let _ = std::fs::copy(out.join("bin").join("physiopulse_ai.dll"), target.join("physiopulse_ai.dll"));
        }
    }
}
