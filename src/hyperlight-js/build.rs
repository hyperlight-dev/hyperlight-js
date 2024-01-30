#![allow(clippy::disallowed_macros)] // allow assert!(..)

// build.rs

// The purpose of this build script is to embed the hyperlight-js-runtime binary as a resource in the hyperlight_js binary.
// This is done by building the hyperlight-js-runtime binary using cargo-hyperlight and reading it into a static byte array
// named JSRUNTIME.
// this build script writes the content of the hyperlight-js-runtime binary to a file named host_resource.rs in the OUT_DIR.
// this file is included in lib.rs.

// The source crate for the hyperlight-js-runtime binary is obtained through cargo metadata, and obtaining the manifest_path
// of the hyperlight-js-runtime dependency.

use std::path::{Path, PathBuf};
use std::{env, fs};

fn main() {
    if env::var("DOCS_RS").is_ok() {
        // docs.rs runs offline, so we can't prepare the sysroot for x86_64-hyperlight-none in there.
        // just bundle an empty resource to make sure the docs build correctly.
        bundle_dummy();
        return;
    }

    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("host_resource.rs");
    let _ = fs::remove_file(&dest_path);

    bundle_runtime();
}

fn resolve_js_runtime_manifest_path() -> PathBuf {
    // Use cargo metadata to obtain information about our dependencies
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = std::process::Command::new(&cargo)
        .args(["metadata", "--format-version=1"])
        .output()
        .expect("Cargo is not installed or not found in PATH");

    assert!(
        output.status.success(),
        "Failed to get cargo metadata: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Cargo metadata output is in JSON format, so we use serde_json to parse it.
    // The output will look like this:
    // {
    //     "packages": [
    //         ...,
    //         {
    //             "name": "hyperlight-js-runtime",
    //             "manifest_path": "/path/to/hyperlight-js-runtime/Cargo.toml",
    //             ...
    //         },
    //         ...
    //     ],
    //     ...
    // }
    // We only care about the name and manifest_path fields of the packages, so we
    // define a minimal struct to deserialize the output.
    #[derive(serde::Deserialize)]
    struct CargoMetadata {
        packages: Vec<CargoPackage>,
    }

    #[derive(serde::Deserialize)]
    struct CargoPackage {
        name: String,
        manifest_path: PathBuf,
    }

    let metadata: CargoMetadata =
        serde_json::from_slice(&output.stdout).expect("Failed to parse cargo metadata");

    // find the package entry for hyperlight-js-runtime and get its manifest_path
    let hyperlight_js_runtime = metadata
        .packages
        .into_iter()
        .find(|pkg| pkg.name == "hyperlight-js-runtime")
        .expect("hyperlight-js-runtime crate not found in cargo metadata");

    hyperlight_js_runtime.manifest_path
}

fn find_target_dir() -> PathBuf {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let out_dir = Path::new(&out_dir);
    let target = env::var("TARGET").unwrap();

    // out_dir is expected to be something like /path/to/target/(ARCH?)/debug/build/hyperlight_js-xxxx/out
    // move up until either ARCH or "target"
    let target_dir = out_dir
        .ancestors()
        .nth(4)
        .expect("OUT_DIR does not have enough ancestors to find target directory");

    // If the target directory is named after the target triple, move up one more level to get to the actual target directory
    // Also, check that the parent directory contains a CACHEDIR.TAG file to make sure we're in the right place
    if target_dir.file_name() == Some(target.as_str().as_ref())
        && let Some(parent) = target_dir.parent()
        && parent.join("CACHEDIR.TAG").exists()
    {
        return parent.to_path_buf();
    }

    target_dir.to_path_buf()
}

fn build_js_runtime() -> PathBuf {
    let profile = env::var_os("PROFILE").unwrap();

    // Get the current target directory.
    let target_dir = find_target_dir();
    // Do not use the target directory directly, as it is locked by cargo with the current build
    // and would result in a deadlock
    let target_dir = target_dir.join("hyperlight-js-runtime");

    let manifest_path = resolve_js_runtime_manifest_path();

    assert!(
        manifest_path.is_file(),
        "expected hyperlight-js-runtime manifest path to be a Cargo.toml file, got {manifest_path:?}",
    );

    let runtime_dir = manifest_path
        .parent()
        .expect("expected hyperlight-js-runtime manifest path to have a parent directory");

    println!("cargo:rerun-if-changed={}", runtime_dir.display());

    // the PROFILE env var unfortunately only gives us 1 bit of "dev or release"
    let cargo_profile = if profile == "debug" { "dev" } else { "release" };

    let stubs_inc = runtime_dir.join("stubs").join("include");
    let cflags = format!("-I{} -D__wasi__=1", stubs_inc.display());

    let mut cargo_cmd = cargo_hyperlight::cargo().unwrap();
    let cmd = cargo_cmd
        .arg("build")
        .arg("--profile")
        .arg(cargo_profile)
        .arg("-v")
        .arg("--target-dir")
        .arg(&target_dir)
        .arg("--manifest-path")
        .arg(manifest_path)
        .arg("--locked")
        .env_clear_cargo()
        .env("HYPERLIGHT_CFLAGS", cflags);

    if std::env::var("CARGO_FEATURE_TRACE_GUEST").is_ok() {
        cmd.arg("--features").arg("trace_guest");
    }

    cmd.status().unwrap_or_else(|e| {
        panic!("Could not run `cargo build` for the js runtime: {e:?}\n{cmd:?}")
    });

    let resource = target_dir
        .join("x86_64-hyperlight-none")
        .join(profile)
        .join("hyperlight-js-runtime");

    if let Ok(path) = resource.canonicalize() {
        path
    } else {
        panic!(
            "could not find hyperlight-js-runtime runtime after building it (expected {:?})",
            resource
        )
    }
}

fn bundle_runtime() {
    let js_runtime_resource = build_js_runtime();

    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("host_resource.rs");
    let contents =
        format!("pub (super) static JSRUNTIME: &[u8] = include_bytes!({js_runtime_resource:?});");

    fs::write(dest_path, contents).unwrap();
    println!("cargo:rerun-if-changed=build.rs");
}

fn bundle_dummy() {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("host_resource.rs");
    let contents = "pub (super) static JSRUNTIME: &[u8] = &[];";
    fs::write(dest_path, contents).unwrap();
}
