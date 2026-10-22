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

#![allow(clippy::disallowed_macros)]

use hyperlight_js::{JSSandbox, LoadedJSSandbox, OciTag, SandboxBuilder, Script, Snapshot};

fn oci_tag(name: &str) -> OciTag {
    OciTag::new(name).unwrap()
}

fn counter_handler() -> Script {
    Script::from_content(
        r#"
        let count = 0;
        function handler(event) {
            count += 1;
            event.count = count;
            return event;
        }
        "#,
    )
}

#[test]
fn persists_and_restores_js_sandbox() {
    let directory = tempfile::tempdir().unwrap();
    let mut sandbox = SandboxBuilder::new()
        .build()
        .unwrap()
        .load_runtime()
        .unwrap();
    sandbox.add_handler("counter", counter_handler()).unwrap();

    let snapshot = sandbox.snapshot().unwrap();
    let digest = snapshot
        .save(directory.path(), &oci_tag("runtime-ready"))
        .unwrap();
    assert!(digest.to_string().starts_with("sha256:"));

    let snapshot =
        Snapshot::<JSSandbox>::checked_load(directory.path(), oci_tag("runtime-ready")).unwrap();
    let sandbox = SandboxBuilder::from_snapshot(snapshot).build().unwrap();
    let mut loaded = sandbox.get_loaded_sandbox().unwrap();

    let result = loaded
        .handle_event("counter", "{}".to_owned(), None)
        .unwrap();
    let result: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(result["count"], 1);
}

#[test]
fn js_sandbox_snapshot_accepts_maximum_oci_tag_length() {
    let directory = tempfile::tempdir().unwrap();
    let mut sandbox = SandboxBuilder::new()
        .build()
        .unwrap()
        .load_runtime()
        .unwrap();
    let snapshot = sandbox.snapshot().unwrap();
    let tag = oci_tag(&"a".repeat(128));

    snapshot.save(directory.path(), &tag).unwrap();

    Snapshot::<JSSandbox>::load(directory.path(), tag).unwrap();
}

#[test]
fn restores_pending_js_sandbox_state_in_memory() {
    let mut sandbox = SandboxBuilder::new()
        .build()
        .unwrap()
        .load_runtime()
        .unwrap();
    sandbox.add_handler("counter", counter_handler()).unwrap();
    let snapshot = sandbox.snapshot().unwrap();

    sandbox.clear_handlers();
    sandbox
        .add_handler(
            "replacement",
            Script::from_content("function handler(event) { return event; }"),
        )
        .unwrap();
    sandbox.restore(snapshot).unwrap();

    let mut loaded = sandbox.get_loaded_sandbox().unwrap();
    let result = loaded
        .handle_event("counter", "{}".to_owned(), None)
        .unwrap();
    let result: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(result["count"], 1);
    assert!(loaded
        .handle_event("replacement", "{}".to_owned(), None)
        .is_err());
}

#[test]
fn rejects_loading_snapshot_as_wrong_lifecycle() {
    let directory = tempfile::tempdir().unwrap();
    let mut sandbox = SandboxBuilder::new()
        .build()
        .unwrap()
        .load_runtime()
        .unwrap();
    sandbox.add_handler("counter", counter_handler()).unwrap();
    sandbox
        .snapshot()
        .unwrap()
        .save(directory.path(), &oci_tag("runtime-ready"))
        .unwrap();
    let mut loaded = sandbox.get_loaded_sandbox().unwrap();
    loaded
        .snapshot()
        .unwrap()
        .save(directory.path(), &oci_tag("application-ready"))
        .unwrap();

    let error =
        match Snapshot::<LoadedJSSandbox>::checked_load(directory.path(), oci_tag("runtime-ready"))
        {
            Ok(_) => panic!("loading a JSSandbox snapshot as loaded should fail"),
            Err(error) => error,
        };
    assert!(error
        .to_string()
        .contains("Snapshot captures a JsSandbox, not a LoadedJsSandbox"));

    let error =
        match Snapshot::<JSSandbox>::checked_load(directory.path(), oci_tag("application-ready")) {
            Ok(_) => panic!("loading a LoadedJSSandbox snapshot as runtime-ready should fail"),
            Err(error) => error,
        };
    assert!(error
        .to_string()
        .contains("Snapshot captures a LoadedJsSandbox, not a JsSandbox"));
}

#[test]
fn persists_loaded_state_and_preserves_unload() {
    let directory = tempfile::tempdir().unwrap();
    let mut sandbox = SandboxBuilder::new()
        .build()
        .unwrap()
        .load_runtime()
        .unwrap();
    sandbox.add_handler("counter", counter_handler()).unwrap();
    let mut loaded = sandbox.get_loaded_sandbox().unwrap();

    let result = loaded
        .handle_event("counter", "{}".to_owned(), None)
        .unwrap();
    let result: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(result["count"], 1);

    let snapshot = loaded.snapshot().unwrap();
    snapshot
        .save(directory.path(), &oci_tag("application-ready"))
        .unwrap();

    let snapshot =
        Snapshot::<LoadedJSSandbox>::checked_load(directory.path(), oci_tag("application-ready"))
            .unwrap();
    let mut loaded = SandboxBuilder::from_snapshot(snapshot).build().unwrap();

    let result = loaded
        .handle_event("counter", "{}".to_owned(), None)
        .unwrap();
    let result: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(result["count"], 2);

    let mut sandbox = loaded.unload().unwrap();
    sandbox.add_handler("counter", counter_handler()).unwrap();
    let mut loaded = sandbox.get_loaded_sandbox().unwrap();
    let result = loaded
        .handle_event("counter", "{}".to_owned(), None)
        .unwrap();
    let result: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(result["count"], 1);
}

#[test]
fn loaded_snapshot_can_be_resaved_to_another_layout() {
    let first_directory = tempfile::tempdir().unwrap();
    let second_directory = tempfile::tempdir().unwrap();
    let mut sandbox = SandboxBuilder::new()
        .build()
        .unwrap()
        .load_runtime()
        .unwrap();
    sandbox.add_handler("counter", counter_handler()).unwrap();
    let mut loaded = sandbox.get_loaded_sandbox().unwrap();

    let result = loaded
        .handle_event("counter", "{}".to_owned(), None)
        .unwrap();
    let result: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(result["count"], 1);

    let snapshot = loaded.snapshot().unwrap();
    let first_digest = snapshot
        .save(first_directory.path(), &oci_tag("application-ready"))
        .unwrap();
    let snapshot =
        Snapshot::<LoadedJSSandbox>::checked_load(first_directory.path(), first_digest.clone())
            .unwrap();
    let second_digest = snapshot
        .save(second_directory.path(), &oci_tag("application-copy"))
        .unwrap();

    assert_eq!(second_digest, first_digest);

    let snapshot =
        Snapshot::<LoadedJSSandbox>::checked_load(second_directory.path(), second_digest).unwrap();
    let mut loaded = SandboxBuilder::from_snapshot(snapshot).build().unwrap();
    let result = loaded
        .handle_event("counter", "{}".to_owned(), None)
        .unwrap();
    let result: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(result["count"], 2);

    let mut sandbox = loaded.unload().unwrap();
    sandbox.add_handler("counter", counter_handler()).unwrap();
    let mut loaded = sandbox.get_loaded_sandbox().unwrap();
    let result = loaded
        .handle_event("counter", "{}".to_owned(), None)
        .unwrap();
    let result: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(result["count"], 1);
}

#[test]
fn loaded_snapshot_rejects_tag_that_would_collide_after_truncation() {
    let directory = tempfile::tempdir().unwrap();
    let mut sandbox = SandboxBuilder::new()
        .build()
        .unwrap()
        .load_runtime()
        .unwrap();
    sandbox.add_handler("counter", counter_handler()).unwrap();
    let mut loaded = sandbox.get_loaded_sandbox().unwrap();
    let snapshot = loaded.snapshot().unwrap();
    let tag = oci_tag(&"a".repeat(119));

    let error = snapshot.save(directory.path(), &tag).unwrap_err();

    assert!(error.to_string().contains("must be at most 118 bytes"));
    assert!(!directory.path().join("index.json").exists());
}

#[test]
fn persisted_js_sandbox_restores_embedded_javascript_modules() {
    let directory = tempfile::tempdir().unwrap();
    let mut sandbox = SandboxBuilder::new()
        .build()
        .unwrap()
        .load_runtime()
        .unwrap();
    sandbox
        .add_module(
            "math",
            Script::from_content("export function double(value) { return value * 2; }"),
        )
        .unwrap();
    sandbox
        .add_handler(
            "double",
            Script::from_content(
                r#"
                import { double } from "user:math";
                function handler(event) { return { value: double(event.value) }; }
                "#,
            ),
        )
        .unwrap();

    let snapshot = sandbox.snapshot().unwrap();
    snapshot
        .save(directory.path(), &oci_tag("embedded-js"))
        .unwrap();
    let snapshot =
        Snapshot::<JSSandbox>::checked_load(directory.path(), oci_tag("embedded-js")).unwrap();
    let mut loaded = SandboxBuilder::from_snapshot(snapshot)
        .build()
        .unwrap()
        .get_loaded_sandbox()
        .unwrap();

    let result = loaded
        .handle_event("double", r#"{"value":21}"#.to_owned(), None)
        .unwrap();
    assert_eq!(result, r#"{"value":42}"#);
}
