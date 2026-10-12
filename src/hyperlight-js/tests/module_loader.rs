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
//! Tests for the module loader that import files from the embedded filesystem.

#![allow(clippy::disallowed_macros)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use hyperlight_js::{
    embed_modules, FileMetadata, FileSystem, FileSystemEmbedded, ResolveError, SandboxBuilder,
    Script, Snapshot,
};

struct CloneCountingFileSystem {
    inner: FileSystemEmbedded,
    clone_count: Arc<AtomicUsize>,
}

impl CloneCountingFileSystem {
    fn new(inner: FileSystemEmbedded, clone_count: Arc<AtomicUsize>) -> Self {
        Self { inner, clone_count }
    }
}

impl Clone for CloneCountingFileSystem {
    fn clone(&self) -> Self {
        self.clone_count.fetch_add(1, Ordering::Relaxed);
        Self {
            inner: self.inner,
            clone_count: Arc::clone(&self.clone_count),
        }
    }
}

impl FileSystem for CloneCountingFileSystem {
    fn new() -> Self {
        unreachable!("CloneCountingFileSystem must wrap an embedded file system")
    }

    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        self.inner.read(path)
    }

    fn read_to_string(&self, path: &Path) -> std::io::Result<String> {
        self.inner.read_to_string(path)
    }

    fn metadata(&self, path: &Path) -> std::io::Result<FileMetadata> {
        self.inner.metadata(path)
    }

    fn symlink_metadata(&self, path: &Path) -> std::io::Result<FileMetadata> {
        self.inner.symlink_metadata(path)
    }

    fn read_link(&self, path: &Path) -> Result<PathBuf, ResolveError> {
        self.inner.read_link(path)
    }

    fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf> {
        self.inner.canonicalize(path)
    }
}

#[test]
fn test_handler_with_multiple_imports() {
    let fs = embed_modules! {
        "math.js" => "fixtures/math.js",
        "strings.js" => "fixtures/strings.js",
    };

    // Create handler that imports both modules
    let handler_content = r#"
    import { add, multiply } from './math.js';
    import { toUpperCase, concat } from './strings.js';

    function handler(event) {
        event.sum = add(event.a, event.b);
        event.product = multiply(event.a, event.b);
        event.message = toUpperCase(concat('Result: ', event.sum));
        return event;
    }
    "#;

    let event = r#"{"a": 5, "b": 3}"#;

    let proto_js_sandbox = SandboxBuilder::new()
        .with_module_loader(fs)
        .build()
        .unwrap();
    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();

    let handler = Script::from_content(handler_content).with_virtual_base("/");
    sandbox.add_handler("calculator", handler).unwrap();

    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();
    let res = loaded_sandbox
        .handle_event("calculator", event.to_string(), None)
        .unwrap();

    assert!(res.contains(r#""sum":8"#));
    assert!(res.contains(r#""product":15"#));
    assert!(res.contains(r#""message":"RESULT: 8"#));
}

#[test]
fn test_handler_import_restrictions() {
    let fs = embed_modules! {
        "math.js" => "fixtures/math.js",
        // strings.js not loaded
    };

    // Create handler that imports both modules
    let handler_content = r#"
    import { add, multiply } from './math.js';
    import { toUpperCase, concat } from './strings.js';

    function handler(event) {
        event.sum = add(event.a, event.b);
        event.product = multiply(event.a, event.b);
        event.message = toUpperCase(concat('Result: ', event.sum));
        return event;
    }
    "#;

    // Compatibility coverage for callers that still configure a fresh ProtoJSSandbox.
    let proto_js_sandbox = SandboxBuilder::new()
        .build()
        .unwrap()
        .set_module_loader(fs)
        .unwrap();
    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();

    let handler = Script::from_content(handler_content).with_virtual_base("/");
    sandbox.add_handler("calculator", handler).unwrap();

    let res = sandbox.get_loaded_sandbox();
    assert!(
        res.is_err(),
        "Expected module not found error for strings.js, got: {:?}",
        res
    );
}

#[test]
fn test_resolve_module_without_resolver_set() {
    let handler_content = r#"
    import { add, multiply } from './math.js';

    function handler(event) {
        event.sum = add(event.a, event.b);
        event.product = multiply(event.a, event.b);
        return event;
    }
    "#;

    let proto_js_sandbox = SandboxBuilder::new().build().unwrap();
    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();

    let handler = Script::from_content(handler_content).with_virtual_base("/");
    sandbox.add_handler("calculator", handler).unwrap();

    // This should fail because we haven't set module loader
    let res = sandbox.get_loaded_sandbox();
    assert!(res.is_err());
}

#[test]
fn test_handler_import_from_a_subfolder() {
    let fs = embed_modules! {
        "hitchhiker.js" => "fixtures/hitchhiker.js",
        "galaxy/deepThought.js" => "fixtures/galaxy/deepThought.js",
        "galaxy/index.js" => "fixtures/galaxy/index.js",
    };

    // Create handler that imports a module which itself imports another module
    let handler_content = r#"
    import { ultimateQuestionOfEverything } from './galaxy/index.js';

    function handler(event) {
        return ultimateQuestionOfEverything;
    }
    "#;

    let proto_js_sandbox = SandboxBuilder::new()
        .with_module_loader(fs)
        .build()
        .unwrap();
    let mut sandbox = proto_js_sandbox.load_runtime().unwrap();

    let handler = Script::from_content(handler_content).with_virtual_base("/");
    sandbox.add_handler("hitchhiker", handler).unwrap();

    let event = r#"{}"#;
    let mut loaded_sandbox = sandbox.get_loaded_sandbox().unwrap();
    let res = loaded_sandbox
        .handle_event("hitchhiker", event.to_string(), None)
        .unwrap();

    assert_eq!(res, "42");
}

#[test]
fn restored_sandbox_reuses_module_resolver() {
    let modules = embed_modules! {
        "math.js" => "fixtures/math.js",
        "strings.js" => "fixtures/strings.js",
    };
    let clone_count = Arc::new(AtomicUsize::new(0));
    let file_system = CloneCountingFileSystem::new(modules, Arc::clone(&clone_count));
    let handler = Script::from_content(
        r#"
        import { add } from './math.js';
        import { toUpperCase } from './strings.js';

        function handler(event) {
            event.sum = add(event.a, event.b);
            event.message = toUpperCase('restored');
            return event;
        }
        "#,
    )
    .with_virtual_base("/");

    let mut sandbox = SandboxBuilder::new()
        .build()
        .unwrap()
        .load_runtime()
        .unwrap();
    sandbox.add_handler("calculator", handler).unwrap();
    let snapshot = sandbox.snapshot().unwrap();

    let restorer = SandboxBuilder::new()
        .with_module_loader(file_system)
        .build_from_snapshot(snapshot)
        .unwrap();
    let clones_after_setup = clone_count.load(Ordering::Relaxed);
    let sandbox = restorer.restore::<hyperlight_js::JSSandbox>().unwrap();
    let mut loaded = sandbox.get_loaded_sandbox().unwrap();

    assert_eq!(clone_count.load(Ordering::Relaxed), clones_after_setup);

    let result = loaded
        .handle_event("calculator", r#"{"a":5,"b":3}"#.to_owned(), None)
        .unwrap();
    assert!(result.contains(r#""sum":8"#));
    assert!(result.contains(r#""message":"RESTORED""#));
}

#[test]
fn restore_reports_required_module_loader() {
    let modules = embed_modules! {
        "math.js" => "fixtures/math.js",
    };
    let proto = SandboxBuilder::new()
        .with_module_loader(modules)
        .build()
        .unwrap();
    let mut sandbox = proto.load_runtime().unwrap();
    let snapshot = sandbox.snapshot().unwrap();
    let restorer = SandboxBuilder::new().build_from_snapshot(snapshot).unwrap();

    let error = match restorer.restore::<hyperlight_js::JSSandbox>() {
        Ok(_) => panic!("restore unexpectedly succeeded without a module loader"),
        Err(error) => error,
    };
    let message = error.to_string();

    assert!(message.contains("Snapshot requires a module loader"));
    assert!(message.contains("with_module_loader()"));
    assert!(message.contains("LoadModule"));
    assert!(message.contains("ResolveModule"));
}

#[test]
fn persisted_snapshot_restores_with_a_replacement_module_loader() {
    let modules = embed_modules! {
        "math.js" => "fixtures/math.js",
    };
    let handler = Script::from_content(
        r#"
        import { add } from './math.js';
        function handler(event) { return { value: add(event.value, 1) }; }
        "#,
    )
    .with_virtual_base("/");
    let proto = SandboxBuilder::new()
        .with_module_loader(modules)
        .build()
        .unwrap();
    let mut sandbox = proto.load_runtime().unwrap();
    sandbox.add_handler("increment", handler).unwrap();
    let snapshot = sandbox.snapshot().unwrap();
    let directory = tempfile::tempdir().unwrap();
    snapshot.save(directory.path(), "external-modules").unwrap();

    let snapshot = Snapshot::load(directory.path(), "external-modules").unwrap();
    let restorer = SandboxBuilder::new()
        .with_module_loader(modules)
        .build_from_snapshot(snapshot)
        .unwrap();
    let mut loaded = restorer
        .restore::<hyperlight_js::JSSandbox>()
        .unwrap()
        .get_loaded_sandbox()
        .unwrap();

    let result = loaded
        .handle_event("increment", r#"{"value":41}"#.to_owned(), None)
        .unwrap();
    assert_eq!(result, r#"{"value":42}"#);
}
