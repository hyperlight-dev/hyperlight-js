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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hyperlight_host::sandbox::snapshot::{
    OciDigest, OciReference, OciTag, Snapshot as HyperlightSnapshot,
};
use serde::{Deserialize, Serialize};

use super::js_sandbox::{validate_module_identifier, validate_namespace_not_reserved};
use crate::{new_error, Result, Script};

const METADATA_NAMESPACE: &str = "dev.hyperlight.hyperlight-js.snapshot.v1";
const METADATA_SCHEMA_VERSION: u32 = 1;
const BASE_TAG_SUFFIX: &str = ".hljs-base";
const MAX_OCI_TAG_LENGTH: usize = 128;
const MAX_LOADED_SNAPSHOT_TAG_LENGTH: usize = MAX_OCI_TAG_LENGTH - BASE_TAG_SUFFIX.len();

/// Logical JavaScript host modules and exported function names.
///
/// Signatures are intentionally not represented. Typed Rust callbacks are
/// erased to a JSON bridge, JavaScript callbacks are dynamic, and Hyperlight
/// can validate only the fixed `CallHostJsFunction` bridge signature. This
/// manifest therefore guarantees name availability, not semantic signature
/// compatibility; callers must keep implementations compatible across restore.
pub(crate) type HostFunctionManifest = BTreeMap<String, Vec<String>>;

/// The Hyperlight JavaScript lifecycle state captured by a [`Snapshot`].
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotKind {
    /// The JavaScript runtime is loaded, with handlers and modules added to the
    /// `JSSandbox` but not yet registered in the guest.
    JsSandbox,
    /// The JavaScript runtime, handlers, and modules are loaded in the guest.
    LoadedJsSandbox,
}

/// Whether a host resource is required by a snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequirementStatus {
    /// The underlying snapshot API does not expose this requirement yet.
    Unknown,
    /// The resource is required to restore the snapshot.
    Required,
    /// The resource is not required to restore the snapshot.
    NotRequired,
}

/// Host resources required to restore a [`Snapshot`].
///
/// This type is non-exhaustive so additional requirements exposed by
/// Hyperlight can be added without changing the existing API.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct SnapshotRequirements {
    host_functions: HostFunctionManifest,
    module_loader: RequirementStatus,
}

impl SnapshotRequirements {
    /// Required Hyperlight-JS host functions grouped by qualified module name.
    pub fn host_functions(&self) -> &BTreeMap<String, Vec<String>> {
        &self.host_functions
    }

    /// Whether the snapshot requires a custom JavaScript module loader.
    ///
    /// This is currently [`RequirementStatus::Unknown`] because Hyperlight
    /// records the requirement internally but does not expose it for
    /// inspection. Restore still validates the requirement before changing
    /// guest state. See <https://github.com/hyperlight-dev/hyperlight/issues/1870>.
    pub fn module_loader(&self) -> RequirementStatus {
        self.module_loader
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScriptMetadata {
    content: Arc<str>,
    base_path: Option<PathBuf>,
}

impl From<&Script> for ScriptMetadata {
    fn from(script: &Script) -> Self {
        Self {
            content: script.shared_content(),
            base_path: script.base_path().map(Path::to_path_buf),
        }
    }
}

impl From<ScriptMetadata> for Script {
    fn from(metadata: ScriptMetadata) -> Self {
        let script = Script::from_shared_content(metadata.content);
        match metadata.base_path {
            Some(path) => script.with_virtual_base(path.to_string_lossy()),
            None => script,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SnapshotMetadata {
    schema_version: u32,
    hyperlight_js_version: String,
    kind: SnapshotKind,
    handlers: BTreeMap<String, ScriptMetadata>,
    modules: BTreeMap<String, ScriptMetadata>,
    host_functions: HostFunctionManifest,
    // Persistence-only OCI digest used to resolve `Snapshot::base` during load.
    // `Snapshot::base` is authoritative in memory; save rewrites this field.
    base_snapshot: Option<String>,
}

impl SnapshotMetadata {
    pub(crate) fn new(
        kind: SnapshotKind,
        handlers: impl Iterator<Item = (String, ScriptMetadata)>,
        modules: impl Iterator<Item = (String, ScriptMetadata)>,
        host_functions: HostFunctionManifest,
    ) -> Self {
        Self {
            schema_version: METADATA_SCHEMA_VERSION,
            hyperlight_js_version: env!("CARGO_PKG_VERSION").to_owned(),
            kind,
            handlers: handlers.collect(),
            modules: modules.collect(),
            host_functions,
            base_snapshot: None,
        }
    }

    pub(crate) fn kind(&self) -> SnapshotKind {
        self.kind
    }

    pub(crate) fn handlers(&self) -> impl Iterator<Item = (String, Script)> + '_ {
        self.handlers
            .clone()
            .into_iter()
            .map(|(name, script)| (name, script.into()))
    }

    pub(crate) fn modules(&self) -> impl Iterator<Item = (String, Script)> + '_ {
        self.modules
            .clone()
            .into_iter()
            .map(|(name, script)| (name, script.into()))
    }

    fn requirements(&self) -> SnapshotRequirements {
        SnapshotRequirements {
            host_functions: self.host_functions.clone(),
            module_loader: RequirementStatus::Unknown,
        }
    }

    /// Validate that the destination sandbox provides every logical host function
    /// required by this guest state. Hyperlight validates only the single typed
    /// `CallHostJsFunction` bridge, so its registry cannot see these JS module
    /// and function names.
    pub(crate) fn validate_host_functions(&self, available: &HostFunctionManifest) -> Result<()> {
        for (module, required_functions) in &self.host_functions {
            let available_functions = available.get(module).ok_or_else(|| {
                new_error!(
                    "Snapshot requires host module '{}', but it was not registered",
                    module
                )
            })?;
            for function in required_functions {
                if !available_functions.contains(function) {
                    return Err(new_error!(
                        "Snapshot requires host function '{}.{}', but it was not registered",
                        module,
                        function
                    ));
                }
            }
        }
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        if self.schema_version != METADATA_SCHEMA_VERSION {
            return Err(new_error!(
                "Unsupported Hyperlight JavaScript snapshot metadata version {} produced by hyperlight-js {}; expected {}",
                self.schema_version,
                self.hyperlight_js_version,
                METADATA_SCHEMA_VERSION
            ));
        }
        match (self.kind, self.base_snapshot.is_some()) {
            (SnapshotKind::JsSandbox, true) => {
                return Err(new_error!(
                    "JSSandbox snapshot metadata must not contain a base snapshot"
                ));
            }
            (SnapshotKind::LoadedJsSandbox, false) => {
                return Err(new_error!(
                    "LoadedJSSandbox snapshot metadata is missing its base snapshot"
                ));
            }
            _ => {}
        }

        if self.handlers.keys().any(|name| name.is_empty()) {
            return Err(new_error!(
                "Snapshot metadata contains an empty handler name"
            ));
        }

        for qualified_name in self.modules.keys() {
            let (namespace, module_name) = qualified_name.split_once(':').ok_or_else(|| {
                new_error!(
                    "Snapshot module name '{}' must be qualified as '<namespace>:<module>'",
                    qualified_name
                )
            })?;
            validate_module_identifier(namespace, "Module namespace")?;
            validate_module_identifier(module_name, "Module name")?;
            validate_namespace_not_reserved(namespace)?;
        }

        Ok(())
    }
}

/// A persistent snapshot of either a `JSSandbox` or `LoadedJSSandbox`.
#[derive(Clone)]
pub struct Snapshot {
    inner: Arc<HyperlightSnapshot>,
    base: Option<Arc<HyperlightSnapshot>>,
    metadata: Arc<SnapshotMetadata>,
}

impl Snapshot {
    pub(crate) fn new(
        inner: Arc<HyperlightSnapshot>,
        base: Option<Arc<HyperlightSnapshot>>,
        metadata: Arc<SnapshotMetadata>,
    ) -> Self {
        Self {
            inner,
            base,
            metadata,
        }
    }

    /// Return the sandbox lifecycle state captured by this snapshot.
    pub fn kind(&self) -> SnapshotKind {
        self.metadata.kind()
    }

    /// Return the host resources required to restore this snapshot.
    pub fn requirements(&self) -> SnapshotRequirements {
        self.metadata.requirements()
    }

    /// Save the snapshot to an OCI image layout and return its manifest digest.
    pub fn save(&self, path: impl AsRef<Path>, tag: impl AsRef<str>) -> Result<String> {
        let tag = OciTag::new(tag.as_ref())?;
        let mut metadata = self.metadata.as_ref().clone();
        if let Some(base) = &self.base {
            // Hyperlight does not yet support OCI snapshot bundles, so save the
            // runtime-ready baseline separately and link it through metadata.
            // https://github.com/hyperlight-dev/hyperlight/issues/1865
            let base_tag = base_tag(&tag)?;
            let base_digest = base.save(path.as_ref(), &base_tag)?;
            metadata.base_snapshot = Some(base_digest.to_string());
        }
        let snapshot = self.inner.with_metadata(METADATA_NAMESPACE, &metadata)?;

        snapshot.save(path, &tag).map(|digest| digest.to_string())
    }

    /// Load and verify a snapshot from an OCI image layout.
    pub fn load(path: impl AsRef<Path>, reference: impl AsRef<str>) -> Result<Self> {
        Self::load_inner(path.as_ref(), reference.as_ref(), true)
    }

    /// Load a trusted snapshot without verifying its OCI blob digests.
    pub fn load_unverified(path: impl AsRef<Path>, reference: impl AsRef<str>) -> Result<Self> {
        Self::load_inner(path.as_ref(), reference.as_ref(), false)
    }

    fn load_inner(path: &Path, reference: &str, verified: bool) -> Result<Self> {
        let reference = reference.parse::<OciReference>()?;
        let inner = Arc::new(if verified {
            HyperlightSnapshot::checked_load(path, reference)?
        } else {
            HyperlightSnapshot::load(path, reference)?
        });
        let metadata = inner
            .metadata::<SnapshotMetadata>(METADATA_NAMESPACE)?
            .ok_or_else(|| {
                new_error!(
                    "Snapshot does not contain Hyperlight JavaScript metadata under namespace '{}'",
                    METADATA_NAMESPACE
                )
            })?;
        metadata.validate()?;

        let base = match &metadata.base_snapshot {
            Some(reference) => {
                let reference = reference.parse::<OciDigest>()?;
                Some(Arc::new(if verified {
                    HyperlightSnapshot::checked_load(path, reference)?
                } else {
                    HyperlightSnapshot::load(path, reference)?
                }))
            }
            None => None,
        };

        Ok(Self {
            inner,
            base,
            metadata: Arc::new(metadata),
        })
    }

    pub(crate) fn inner(&self) -> Arc<HyperlightSnapshot> {
        Arc::clone(&self.inner)
    }

    pub(crate) fn base(&self) -> Option<Arc<HyperlightSnapshot>> {
        self.base.clone()
    }

    pub(crate) fn metadata(&self) -> &SnapshotMetadata {
        &self.metadata
    }
}

fn base_tag(tag: &OciTag) -> Result<OciTag> {
    // TODO(https://github.com/hyperlight-dev/hyperlight/issues/1865): Remove
    // this shorter limit when both states are saved as one OCI bundle.
    if tag.as_str().len() > MAX_LOADED_SNAPSHOT_TAG_LENGTH {
        return Err(new_error!(
            "LoadedJSSandbox snapshot tag must be at most {} bytes to reserve space for its runtime-ready baseline tag",
            MAX_LOADED_SNAPSHOT_TAG_LENGTH
        ));
    }

    OciTag::new(format!("{}{BASE_TAG_SUFFIX}", tag.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(kind: SnapshotKind) -> SnapshotMetadata {
        SnapshotMetadata::new(
            kind,
            std::iter::empty(),
            std::iter::empty(),
            BTreeMap::new(),
        )
    }

    fn script_metadata() -> ScriptMetadata {
        ScriptMetadata::from(&Script::from_content("function handler() {}"))
    }

    #[test]
    fn metadata_rejects_unknown_fields() {
        let mut value = serde_json::to_value(metadata(SnapshotKind::JsSandbox)).unwrap();
        value["unknown"] = serde_json::json!(true);

        let error = serde_json::from_value::<SnapshotMetadata>(value).unwrap_err();

        assert!(error.to_string().contains("unknown field `unknown`"));
    }

    #[test]
    fn script_metadata_rejects_unknown_fields() {
        let mut value = serde_json::to_value(script_metadata()).unwrap();
        value["unknown"] = serde_json::json!(true);

        let error = serde_json::from_value::<ScriptMetadata>(value).unwrap_err();

        assert!(error.to_string().contains("unknown field `unknown`"));
    }

    #[test]
    fn metadata_schema_error_includes_producer_version() {
        let mut metadata = metadata(SnapshotKind::JsSandbox);
        metadata.schema_version = METADATA_SCHEMA_VERSION + 1;
        metadata.hyperlight_js_version = "9.8.7".to_owned();

        let error = metadata.validate().unwrap_err();

        assert!(error
            .to_string()
            .contains("produced by hyperlight-js 9.8.7"));
    }

    #[test]
    fn loaded_metadata_does_not_require_staged_definitions() {
        let mut metadata = metadata(SnapshotKind::LoadedJsSandbox);
        metadata.base_snapshot = Some(format!("sha256:{}", "0".repeat(64)));

        metadata.validate().unwrap();
    }

    #[test]
    fn metadata_rejects_empty_handler_name() {
        let mut metadata = metadata(SnapshotKind::JsSandbox);
        metadata.handlers.insert(String::new(), script_metadata());

        let error = metadata.validate().unwrap_err();

        assert!(error.to_string().contains("empty handler name"));
    }

    #[test]
    fn metadata_rejects_invalid_qualified_module_names() {
        for name in [
            "module",
            ":module",
            "user:",
            "user:module:extra",
            "host:module",
        ] {
            let mut metadata = metadata(SnapshotKind::JsSandbox);
            metadata.modules.insert(name.to_owned(), script_metadata());

            assert!(
                metadata.validate().is_err(),
                "accepted module name {name:?}"
            );
        }
    }

    #[test]
    fn metadata_accepts_valid_public_api_identifiers() {
        let mut metadata = metadata(SnapshotKind::JsSandbox);
        metadata
            .handlers
            .insert("handler:name".to_owned(), script_metadata());
        metadata
            .modules
            .insert("user:module".to_owned(), script_metadata());

        metadata.validate().unwrap();
    }

    #[test]
    fn requirements_expose_host_functions_without_guessing_module_loader() {
        let mut metadata = metadata(SnapshotKind::LoadedJsSandbox);
        metadata.host_functions.insert(
            "host:database".to_owned(),
            vec!["execute".to_owned(), "query".to_owned()],
        );

        let requirements = metadata.requirements();

        assert_eq!(
            requirements.host_functions().get("host:database"),
            Some(&vec!["execute".to_owned(), "query".to_owned()])
        );
        assert_eq!(requirements.module_loader(), RequirementStatus::Unknown);
    }

    #[test]
    fn script_metadata_shares_source_and_preserves_json_shape() {
        let script =
            Script::from_content("export const answer = 42;").with_virtual_base("/modules");
        let original_content = script.shared_content();
        let metadata = ScriptMetadata::from(&script);

        let json = serde_json::to_value(&metadata).unwrap();
        let restored_metadata: ScriptMetadata = serde_json::from_value(json.clone()).unwrap();
        let restored_script: Script = metadata.into();

        assert!(Arc::ptr_eq(
            &original_content,
            &restored_script.shared_content()
        ));
        assert_eq!(json["content"], "export const answer = 42;");
        assert_eq!(json["base_path"], "/modules");
        assert_eq!(
            restored_metadata.content.as_ref(),
            "export const answer = 42;"
        );
    }

    #[test]
    fn base_tag_accepts_maximum_loaded_snapshot_tag_length() {
        let tag = OciTag::new("a".repeat(MAX_LOADED_SNAPSHOT_TAG_LENGTH)).unwrap();

        let base = base_tag(&tag).unwrap();

        assert_eq!(base.as_str().len(), MAX_OCI_TAG_LENGTH);
        assert_eq!(base.as_str(), format!("{}{BASE_TAG_SUFFIX}", tag.as_str()));
    }

    #[test]
    fn base_tag_rejects_tag_that_would_require_truncation() {
        let tag = OciTag::new("a".repeat(MAX_LOADED_SNAPSHOT_TAG_LENGTH + 1)).unwrap();

        let error = base_tag(&tag).unwrap_err();

        assert!(error.to_string().contains("must be at most 118 bytes"));
    }

    #[test]
    fn distinct_loaded_snapshot_tags_have_distinct_base_tags() {
        let shared_prefix = "a".repeat(MAX_LOADED_SNAPSHOT_TAG_LENGTH - 1);
        let first = OciTag::new(format!("{shared_prefix}x")).unwrap();
        let second = OciTag::new(format!("{shared_prefix}y")).unwrap();

        assert_ne!(base_tag(&first).unwrap(), base_tag(&second).unwrap());
    }
}
