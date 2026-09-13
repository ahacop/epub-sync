//! Builds the Go shim around kepubify as a static C archive and links it
//! into this crate. Go is a build-time dependency only.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let shim_dir = manifest_dir.join("../../kepub-shim");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let archive = out_dir.join("libkepubshim.a");

    println!(
        "cargo:rerun-if-changed={}",
        shim_dir.join("main.go").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        shim_dir.join("go.mod").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        shim_dir.join("go.sum").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        shim_dir.join("vendor").display()
    );

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let goos = match target_os.as_str() {
        "macos" => "darwin",
        other => other,
    };
    let goarch = match target_arch.as_str() {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    };

    let status = Command::new("go")
        .args([
            "build",
            "-mod=vendor",
            "-buildmode=c-archive",
            "-trimpath",
            "-o",
        ])
        .arg(&archive)
        .arg(".")
        .current_dir(&shim_dir)
        .env("GOOS", goos)
        .env("GOARCH", goarch)
        .env("CGO_ENABLED", "1")
        .status();

    match status {
        Ok(s) if s.success() => {}
        Ok(s) => panic!("go build of kepub-shim failed with {s}"),
        Err(e) => panic!(
            "cannot run `go` ({e}). Go is a build dependency of epubsync-core: \
             it compiles the kepubify shim in kepub-shim/. Install Go and put it on PATH."
        ),
    }

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=kepubshim");
    if target_os == "macos" {
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
        println!("cargo:rustc-link-lib=framework=Security");
    }
}
