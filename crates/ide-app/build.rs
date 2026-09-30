use std::env;
use std::path::PathBuf;
use std::process::Command;

fn run(command: &mut Command, label: &str) {
    let status = command
        .status()
        .unwrap_or_else(|error| panic!("could not start {label}: {error}"));
    assert!(status.success(), "{label} failed with {status}");
}

fn main() {
    println!("cargo:rerun-if-changed=src/app_update/sparkle_bridge.m");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is required"));
    println!("cargo:rerun-if-changed=assets/studio/thumbnail.swift");
    let swift_arch = if env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("aarch64") {
        "arm64"
    } else {
        "x86_64"
    };
    let swift_target = format!("{swift_arch}-apple-macosx13.0");
    run(
        Command::new("xcrun")
            .args([
                "swiftc",
                "-target",
                &swift_target,
                "-O",
                "-framework",
                "AppKit",
                "-framework",
                "WebKit",
                "assets/studio/thumbnail.swift",
                "-o",
            ])
            .arg(out_dir.join("choro-studio-thumbnail")),
        "Studio thumbnail helper compilation",
    );
    let object = out_dir.join("sparkle_bridge.o");
    let library = out_dir.join("libchoro_sparkle_bridge.a");

    run(
        Command::new("xcrun")
            .args([
                "--sdk",
                "macosx",
                "clang",
                "-fobjc-arc",
                "-fblocks",
                "-Werror",
                "-Wall",
                "-Wextra",
                "-Wno-objc-method-access",
                "-mmacosx-version-min=13.0",
                "-c",
                "src/app_update/sparkle_bridge.m",
                "-o",
            ])
            .arg(&object),
        "Objective-C Sparkle bridge compilation",
    );
    run(
        Command::new("xcrun")
            .args(["libtool", "-static", "-o"])
            .arg(&library)
            .arg(&object),
        "Objective-C Sparkle bridge archive",
    );

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=choro_sparkle_bridge");
    println!("cargo:rustc-link-lib=framework=Foundation");
}
