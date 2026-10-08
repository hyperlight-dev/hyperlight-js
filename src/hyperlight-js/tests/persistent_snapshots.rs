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

use hyperlight_js::{JSSandbox, LoadedJSSandbox, SandboxBuilder, Script, Snapshot, SnapshotKind};

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
    assert_eq!(snapshot.kind(), SnapshotKind::JsSandbox);
    let digest = snapshot.save(directory.path(), "runtime-ready").unwrap();
    assert!(digest.starts_with("sha256:"));

    let snapshot = Snapshot::load(directory.path(), "runtime-ready").unwrap();
    let restorer = SandboxBuilder::new().build_from_snapshot(snapshot).unwrap();
    let sandbox = restorer.restore::<JSSandbox>().unwrap();
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
    let tag = "a".repeat(128);

    snapshot.save(directory.path(), &tag).unwrap();

    let snapshot = Snapshot::load(directory.path(), &tag).unwrap();
    assert_eq!(snapshot.kind(), SnapshotKind::JsSandbox);
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
fn rejects_wrong_persisted_snapshot_lifecycle_restore() {
    let mut sandbox = SandboxBuilder::new()
        .build()
        .unwrap()
        .load_runtime()
        .unwrap();
    sandbox.add_handler("counter", counter_handler()).unwrap();
    let js_snapshot = sandbox.snapshot().unwrap();
    let mut loaded = sandbox.get_loaded_sandbox().unwrap();
    let loaded_snapshot = loaded.snapshot().unwrap();

    let restorer = SandboxBuilder::new()
        .build_from_snapshot(js_snapshot)
        .unwrap();
    let error = match restorer.restore::<LoadedJSSandbox>() {
        Ok(_) => panic!("restoring a JSSandbox snapshot as loaded should fail"),
        Err(error) => error,
    };
    assert!(error
        .to_string()
        .contains("Cannot restore a JsSandbox snapshot as a LoadedJSSandbox"));

    let restorer = SandboxBuilder::new()
        .build_from_snapshot(loaded_snapshot)
        .unwrap();
    let error = match restorer.restore::<JSSandbox>() {
        Ok(_) => panic!("restoring a LoadedJSSandbox snapshot as runtime-ready should fail"),
        Err(error) => error,
    };
    assert!(error
        .to_string()
        .contains("Cannot restore a LoadedJsSandbox snapshot as a JSSandbox"));
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
    assert_eq!(snapshot.kind(), SnapshotKind::LoadedJsSandbox);
    snapshot
        .save(directory.path(), "application-ready")
        .unwrap();

    let snapshot = Snapshot::load(directory.path(), "application-ready").unwrap();
    let restorer = SandboxBuilder::new().build_from_snapshot(snapshot).unwrap();
    let mut loaded = restorer.restore::<LoadedJSSandbox>().unwrap();

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
        .save(first_directory.path(), "application-ready")
        .unwrap();
    let snapshot = Snapshot::load(first_directory.path(), &first_digest).unwrap();
    let second_digest = snapshot
        .save(second_directory.path(), "application-copy")
        .unwrap();

    assert_eq!(second_digest, first_digest);

    let snapshot = Snapshot::load(second_directory.path(), &second_digest).unwrap();
    let restorer = SandboxBuilder::new().build_from_snapshot(snapshot).unwrap();
    let mut loaded = restorer.restore::<LoadedJSSandbox>().unwrap();
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
    let tag = "a".repeat(119);

    let error = snapshot.save(directory.path(), tag).unwrap_err();

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
    snapshot.save(directory.path(), "embedded-js").unwrap();
    let snapshot = Snapshot::load(directory.path(), "embedded-js").unwrap();
    let restorer = SandboxBuilder::new().build_from_snapshot(snapshot).unwrap();
    let mut loaded = restorer
        .restore::<JSSandbox>()
        .unwrap()
        .get_loaded_sandbox()
        .unwrap();

    let result = loaded
        .handle_event("double", r#"{"value":21}"#.to_owned(), None)
        .unwrap();
    assert_eq!(result, r#"{"value":42}"#);
}
