/*
Copyright 2026  The Hyperlight Authors.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/
#![allow(clippy::disallowed_macros)] // allow assert!(..)

// build.rs

// The purpose of this build script is to embed the hyperlight-js-runtime binary as a resource in the hyperlight_js binary.
// This is done by building the hyperlight-js-runtime binary using cargo-hyperlight and reading it into a static byte array
// named JSRUNTIME.
// this build script writes the content of the hyperlight-js-runtime binary to a file named host_resource.rs in the OUT_DIR.
// this file is included in lib.rs.

// The source crate for the hyperlight-js-runtime binary is obtained through cargo metadata, and obtaining the manifest_path
// of the hyperlight-js-runtime dependency.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::{env, fs};

use serde_json::Value;

// cargo-hyperlight supplies libc headers. QuickJS still needs threading disabled
// and the monotonic clock definitions enabled.
const QUICKJS_CFLAGS: &str = "-D__wasi__=1 -D_POSIX_MONOTONIC_CLOCK";

#[derive(Debug, PartialEq)]
pub(crate) enum RuntimeSource {
    Default,
    Manifest { path: PathBuf },
}

pub(crate) fn runtime_source(
    removed_binary_override: Option<OsString>,
    manifest: Option<OsString>,
) -> Result<RuntimeSource, String> {
    let nonempty = |value: &OsString| !value.to_string_lossy().trim().is_empty();
    if removed_binary_override.filter(nonempty).is_some() {
        return Err(
            "HYPERLIGHT_JS_RUNTIME_PATH is no longer supported; unset it and set HYPERLIGHT_JS_RUNTIME_MANIFEST_PATH to your custom runtime's Cargo.toml"
                .into(),
        );
    }
    if let Some(path) = manifest.filter(nonempty) {
        return Ok(RuntimeSource::Manifest { path: path.into() });
    }
    Ok(RuntimeSource::Default)
}

pub(crate) fn select_binary(package: &Value) -> Result<String, String> {
    let targets = package["targets"]
        .as_array()
        .ok_or("Guest package has no targets in cargo metadata")?;
    let binaries: Vec<&str> = targets
        .iter()
        .filter(|target| {
            target["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
        })
        .filter_map(|target| target["name"].as_str())
        .collect();
    match binaries.as_slice() {
        [name] => Ok((*name).to_owned()),
        _ => Err("Runtime manifest must define exactly one binary target".into()),
    }
}

fn main() {
    // Mirror the hypervisor cfg aliases used by hyperlight-host so that
    // `#[cfg(kvm)]` etc. mean "feature enabled *and* on the platform that
    // provides that hypervisor". Never use `#[cfg(feature = "kvm")]` directly.
    cfg_aliases::cfg_aliases! {
        kvm: { all(feature = "kvm", target_os = "linux") },
        mshv3: { all(feature = "mshv3", target_os = "linux") },
        hvf: { all(feature = "hvf", target_os = "macos") },
        whp: { target_os = "windows" },
        // hyperlight-host only implements crash dumps and the gdb debug stub on
        // x86_64, so mirror its aliases — otherwise enabling either feature on
        // aarch64 (e.g. macOS) would expose a wrapper around a method that does
        // not exist.
        crashdump: { all(feature = "crashdump", target_arch = "x86_64") },
        gdb: { all(feature = "gdb", debug_assertions, target_arch = "x86_64") },
    }

    if env::var("DOCS_RS").is_ok() {
        // docs.rs runs offline, so we can't prepare the sysroot for the guest target in there.
        // just bundle an empty resource to make sure the docs build correctly.
        bundle_dummy();
        return;
    }

    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("host_resource.rs");
    let _ = fs::remove_file(&dest_path);

    bundle_runtime();
}

fn read_cargo_metadata(manifest_path: Option<&Path>) -> Value {
    // Inspect the custom guest when supplied, otherwise the host dependency graph.
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut command = std::process::Command::new(&cargo);
    command.args(["metadata", "--format-version=1"]);
    if let Some(path) = manifest_path {
        command.arg("--manifest-path").arg(path);
    }
    let output = command
        .output()
        .expect("Cargo is not installed or not found in PATH");

    assert!(
        output.status.success(),
        "Failed to get cargo metadata: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    serde_json::from_slice(&output.stdout).expect("Failed to parse cargo metadata")
}

fn resolve_js_runtime_manifest_path(metadata: &Value) -> PathBuf {
    // Reuse the host metadata to locate the default runtime. The same response
    // also supplies binary targets and local dependencies for the guest build.
    let hyperlight_js_runtime = metadata["packages"]
        .as_array()
        .expect("Missing packages in cargo metadata")
        .iter()
        .find(|pkg| pkg["name"] == "hyperlight-js-runtime")
        .expect("hyperlight-js-runtime crate not found in cargo metadata");

    PathBuf::from(
        hyperlight_js_runtime["manifest_path"]
            .as_str()
            .expect("Missing hyperlight-js-runtime manifest path in cargo metadata"),
    )
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

/// The hyperlight guest target triple to build the JS runtime for.
///
/// The guest runs inside the VM on the same architecture as the host, so this is
/// derived from the host crate's target arch rather than hardcoded. We pass it to
/// `cargo hyperlight` explicitly instead of relying on its default, because its
/// default is the arch of the `cargo-hyperlight` binary itself, which need not
/// match the arch we are building the host for.
fn guest_target() -> String {
    let arch = env::var("CARGO_CFG_TARGET_ARCH").expect("CARGO_CFG_TARGET_ARCH is not set");
    assert!(
        matches!(arch.as_str(), "x86_64" | "aarch64"),
        "unsupported host architecture for hyperlight-js: {arch}"
    );
    format!("{arch}-hyperlight-none")
}

fn build_js_runtime(custom: Option<PathBuf>) -> PathBuf {
    let profile = env::var_os("PROFILE").unwrap();
    let target = guest_target();

    // Get the current target directory.
    let target_dir = find_target_dir();
    // Do not use the target directory directly, as it is locked by cargo with the current build
    // and would result in a deadlock
    let target_dir = target_dir.join("hyperlight-js-runtime");

    let is_custom = custom.is_some();
    let metadata = read_cargo_metadata(custom.as_deref());
    let manifest_path = custom.unwrap_or_else(|| resolve_js_runtime_manifest_path(&metadata));
    let manifest_path = manifest_path
        .canonicalize()
        .expect("JS runtime manifest must point to an existing Cargo.toml");

    assert!(
        manifest_path.is_file(),
        "expected hyperlight-js-runtime manifest path to be a Cargo.toml file, got {manifest_path:?}",
    );

    let runtime_dir = manifest_path
        .parent()
        .expect("expected hyperlight-js-runtime manifest path to have a parent directory");

    let packages = metadata["packages"].as_array().expect("Missing packages");
    let package = packages
        .iter()
        .find(|package| {
            package["manifest_path"]
                .as_str()
                .and_then(|path| Path::new(path).canonicalize().ok())
                .as_ref()
                == Some(&manifest_path)
        })
        .expect("Custom runtime manifest must identify a package, not a virtual workspace");
    let bin = select_binary(package).unwrap_or_else(|error| panic!("{error}"));

    // Track local dependencies too, including native modules outside the guest crate.
    // Do not watch entire crate directories: they may contain the nested build output.
    for package in packages
        .iter()
        .filter(|package| package["source"].is_null())
    {
        let manifest = Path::new(package["manifest_path"].as_str().unwrap());
        let dir = manifest.parent().unwrap();
        println!("cargo:rerun-if-changed={}", manifest.display());
        for entry in ["src", "build.rs", ".cargo"] {
            let path = dir.join(entry);
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
        for target in package["targets"].as_array().unwrap() {
            println!(
                "cargo:rerun-if-changed={}",
                target["src_path"].as_str().unwrap()
            );
        }
    }
    let workspace = Path::new(metadata["workspace_root"].as_str().unwrap());
    for entry in ["Cargo.toml", "Cargo.lock", ".cargo"] {
        let path = workspace.join(entry);
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }

    // the PROFILE env var unfortunately only gives us 1 bit of "dev or release"
    let cargo_profile = if profile == "debug" { "dev" } else { "release" };

    // Use the runtime dependency's headers, not the custom guest's directory.
    // The math shim must precede the sysroot headers on aarch64.
    let runtime_package = packages
        .iter()
        .find(|package| package["name"] == "hyperlight-js-runtime")
        .expect("hyperlight-js-runtime crate not found in cargo metadata");
    let include_dir = Path::new(runtime_package["manifest_path"].as_str().unwrap())
        .parent()
        .unwrap()
        .join("include")
        .canonicalize()
        .expect("hyperlight-js-runtime include directory not found");
    println!("cargo:rerun-if-changed={}", include_dir.display());
    let cflags = format!("-I{} {QUICKJS_CFLAGS}", include_dir.display()).replace("\\", "\\\\");

    let mut cargo_cmd = cargo_hyperlight::cargo().unwrap();
    let cmd = cargo_cmd
        .arg("build")
        .arg("--profile")
        .arg(cargo_profile)
        .arg("--bin")
        .arg(&bin)
        .arg("--target")
        .arg(&target)
        // The host Cargo process holds its target directory locked. Build the
        // guest separately to avoid a deadlock; cargo-hyperlight forwards this flag.
        .arg("--target-dir")
        .arg(&target_dir)
        .arg("--manifest-path")
        .arg(&manifest_path)
        .arg("--locked")
        .env_clear_cargo()
        .current_dir(runtime_dir)
        .env("HYPERLIGHT_CFLAGS", cflags);

    // Link arguments from the runtime library's build.rs do not propagate to
    // downstream binaries. Preserve its clock override for custom guest builds.
    if is_custom {
        let mut flags = env::var_os("RUSTFLAGS").unwrap_or_default();
        flags.push(" -Clink-arg=--wrap=clock_gettime");
        cmd.env("RUSTFLAGS", flags);
    }
    if std::env::var("CARGO_FEATURE_TRACE_GUEST").is_ok() {
        cmd.arg("--features").arg(if is_custom {
            "hyperlight-js-runtime/trace_guest"
        } else {
            "trace_guest"
        });
    }

    cmd.status().unwrap_or_else(|e| {
        panic!("Could not run `cargo build` for the js runtime: {e:?}\n{cmd:?}")
    });

    let resource = target_dir.join(target).join(profile).join(bin);

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
    // Always rerun if the environment variable changes, even if it's currently unset.
    println!("cargo:rerun-if-env-changed=HYPERLIGHT_JS_RUNTIME_PATH");
    println!("cargo:rerun-if-env-changed=HYPERLIGHT_JS_RUNTIME_MANIFEST_PATH");

    // Relative manifest paths resolve from this build script's working directory
    // (the hyperlight-js crate root), not the invoking host project. Prefer an
    // absolute path. build_js_runtime canonicalizes it and requires it to exist.
    let source = runtime_source(
        env::var_os("HYPERLIGHT_JS_RUNTIME_PATH"),
        env::var_os("HYPERLIGHT_JS_RUNTIME_MANIFEST_PATH"),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let js_runtime_resource = match source {
        RuntimeSource::Manifest { path } => build_js_runtime(Some(path)),
        RuntimeSource::Default => build_js_runtime(None),
    };

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
