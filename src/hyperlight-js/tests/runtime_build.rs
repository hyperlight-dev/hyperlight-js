/*
Copyright 2026 The Hyperlight Authors.

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

#![allow(clippy::disallowed_macros)]

#[allow(dead_code)]
#[path = "../build.rs"]
mod build_script;

use build_script::{runtime_source, select_binary, RuntimeSource};
use serde_json::json;

#[test]
fn default_and_empty_overrides_preserve_embedded_runtime() {
    assert_eq!(
        runtime_source(None, None, None).unwrap(),
        RuntimeSource::Default
    );
    assert_eq!(
        runtime_source(Some(" ".into()), Some("".into()), Some(" ".into())).unwrap(),
        RuntimeSource::Default
    );
}

#[test]
fn removed_binary_override_is_rejected_instead_of_silently_using_another_runtime() {
    for manifest in [None, Some("Cargo.toml".into())] {
        let error = runtime_source(Some("guest".into()), manifest, None).unwrap_err();
        assert!(error.contains("HYPERLIGHT_JS_RUNTIME_PATH is no longer supported"));
        assert!(error.contains("HYPERLIGHT_JS_RUNTIME_MANIFEST_PATH"));
    }
}

#[test]
fn manifest_and_optional_binary_are_selected() {
    assert_eq!(
        runtime_source(None, Some("Cargo.toml".into()), Some("custom".into())).unwrap(),
        RuntimeSource::Manifest {
            path: "Cargo.toml".into(),
            bin: Some("custom".into())
        }
    );
    assert!(runtime_source(None, None, Some("custom".into())).is_err());
}

#[test]
fn binary_selection_ignores_libraries_and_build_scripts() {
    let package = json!({"targets": [
        {"name": "build-script-build", "kind": ["custom-build"]},
        {"name": "runtime_lib", "kind": ["lib"]},
        {"name": "different-from-package-name", "kind": ["bin"]}
    ]});
    assert_eq!(
        select_binary(&package, None).unwrap(),
        "different-from-package-name"
    );
    assert!(select_binary(&package, Some("missing")).is_err());
}

#[test]
fn ambiguous_binaries_require_selection() {
    let mut package = json!({"targets": [
        {"name": "one", "kind": ["bin"]}, {"name": "two", "kind": ["bin"]}
    ]});
    assert!(select_binary(&package, None).is_err());
    package["default_run"] = json!("two");
    assert_eq!(select_binary(&package, None).unwrap(), "two");
    assert_eq!(select_binary(&package, Some("one")).unwrap(), "one");
    assert!(select_binary(&json!({"targets": []}), None).is_err());
}
