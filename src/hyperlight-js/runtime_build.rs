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

use std::ffi::OsString;
use std::path::PathBuf;

use serde_json::Value;

#[derive(Debug, PartialEq)]
pub(crate) enum RuntimeSource {
    Default,
    Manifest { path: PathBuf },
}

pub(crate) fn runtime_source(manifest: Option<OsString>) -> RuntimeSource {
    let nonempty = |value: &OsString| !value.to_string_lossy().trim().is_empty();
    if let Some(path) = manifest.filter(nonempty) {
        return RuntimeSource::Manifest { path: path.into() };
    }
    RuntimeSource::Default
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{runtime_source, select_binary, RuntimeSource};

    #[test]
    fn absent_or_empty_manifest_selects_default_source() {
        for manifest in [None, Some("".into()), Some(" ".into())] {
            assert_eq!(runtime_source(manifest), RuntimeSource::Default);
        }
    }

    #[test]
    fn manifest_selects_custom_source() {
        assert_eq!(
            runtime_source(Some("Cargo.toml".into())),
            RuntimeSource::Manifest {
                path: "Cargo.toml".into()
            }
        );
    }

    #[test]
    fn binary_selection_ignores_libraries_and_build_scripts() {
        let package = json!({"targets": [
            {"name": "build-script-build", "kind": ["custom-build"]},
            {"name": "runtime_lib", "kind": ["lib"]},
            {"name": "different-from-package-name", "kind": ["bin"]}
        ]});
        assert_eq!(
            select_binary(&package).unwrap(),
            "different-from-package-name"
        );
    }

    #[test]
    fn runtime_requires_exactly_one_binary() {
        for package in [
            json!({"targets": [{"name": "runtime_lib", "kind": ["lib"]}]}),
            json!({"targets": [
                {"name": "one", "kind": ["bin"]}, {"name": "two", "kind": ["bin"]}
            ]}),
            json!({"default_run": "two", "targets": [
                {"name": "one", "kind": ["bin"]}, {"name": "two", "kind": ["bin"]}
            ]}),
        ] {
            assert_eq!(
                select_binary(&package).unwrap_err(),
                "Runtime manifest must define exactly one binary target"
            );
        }
    }
}
