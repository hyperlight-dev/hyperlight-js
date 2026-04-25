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

//! Test to verify that tracing features are configured correctly.
//!
//! This test ensures that `release_max_level_error` is NOT enabled for the
//! tracing crate in hyperlight-js-runtime. Having this feature enabled would
//! suppress all log levels above error in release builds, which prevents
//! tracing from working correctly.
//!
//! See: https://github.com/hyperlight-dev/hyperlight-js/issues/126

#![cfg(not(hyperlight))]

/// Verifies that the `release_max_level_error` feature is not enabled for
/// the tracing crate when building hyperlight-js-runtime.
///
/// This is important because enabling `release_max_level_error` would
/// cause all trace/debug/info/warn log levels to be compiled out in
/// release builds, breaking the tracing functionality.
#[test]
fn tracing_does_not_have_release_max_level_error() {
    let mut cmd = std::process::Command::new(env!("CARGO"));
    let output = cmd
        .arg("tree")
        .arg("-p")
        .arg("hyperlight-js-runtime")
        .arg("-f")
        .arg("{f}")
        .arg("-i")
        .arg("tracing")
        .arg("--depth")
        .arg("0")
        .env("RUSTFLAGS", "--cfg=hyperlight --check-cfg=cfg(hyperlight)")
        .output()
        .expect("Failed to run cargo hyperlight tree");

    assert!(
        output.status.success(),
        "cargo hyperlight tree failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let features: Vec<&str> = stdout.trim().split(',').collect();

    assert!(
        !features.contains(&"release_max_level_error"),
        "tracing crate should NOT have 'release_max_level_error' feature enabled.\n\
         Enabled features: {stdout}\n\
         See: https://github.com/hyperlight-dev/hyperlight-js/issues/126"
    );

    // Also verify that the expected features ARE enabled
    assert!(
        features.contains(&"max_level_trace"),
        "tracing crate should have 'max_level_trace' feature enabled.\n\
         Enabled features: {stdout}"
    );
}
