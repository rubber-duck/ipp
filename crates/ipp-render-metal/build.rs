//! Build the Metal device bridge with the macOS SDK.

use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=src/device.m");
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }

    let target = env::var("TARGET").expect("Cargo target triple");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    let object = out.join("device.o");
    let archive = out.join("libipp_metal_device.a");

    let mut clang = Command::new("xcrun");
    clang.args([
        "--sdk",
        "macosx",
        "clang",
        "-target",
        &target,
        "-fobjc-arc",
        "-O2",
        "-Wall",
        "-Wextra",
        "-Werror",
        "-c",
        "src/device.m",
        "-o",
    ]);
    clang.arg(&object);
    if env::var_os("CARGO_FEATURE_INSTRUMENTATION").is_some() {
        clang.arg("-DIPP_METAL_INSTRUMENTATION");
    }
    assert!(
        clang.status().expect("macOS SDK clang").success(),
        "compile Metal device"
    );

    let mut ar = Command::new("xcrun");
    ar.args(["ar", "crs"]).arg(&archive).arg(&object);
    assert!(
        ar.status().expect("macOS SDK ar").success(),
        "archive Metal device"
    );

    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=ipp_metal_device");
    println!("cargo:rustc-link-lib=framework=Metal");
    println!("cargo:rustc-link-lib=framework=IOSurface");
    println!("cargo:rustc-link-lib=framework=Foundation");
}
