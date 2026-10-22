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
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hyperlight_host::sandbox::snapshot::{
    OciDigest, OciReference, OciTag, Snapshot as HyperlightSnapshot,
};
use hyperlight_host::Sandbox;
use serde::{Deserialize, Serialize};

use super::js_sandbox::{validate_module_identifier, validate_namespace_not_reserved};
use crate::{new_error, JSSandbox, LoadedJSSandbox, Result, Script};

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

/// The Hyperlight JavaScript lifecycle state recorded in snapshot metadata.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotKind {
    /// The JavaScript runtime is loaded, with handlers and modules added to the
    /// `JSSandbox` but not yet registered in the guest.
    JsSandbox,
    /// The JavaScript runtime, handlers, and modules are loaded in the guest.
    LoadedJsSandbox,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScriptMetadata {
    content: Arc<String>,
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

mod private {
    pub trait Sealed {
        const KIND: super::SnapshotKind;

        fn from_snapshot(
            inner: super::Sandbox,
            snapshot: &super::Snapshot<Self>,
            host_functions: super::HostFunctionManifest,
        ) -> super::Result<Self>
        where
            Self: Sized;
    }
}

/// A sandbox type that a [`Snapshot`] can capture and restore.
///
/// This trait is sealed and implemented only for [`JSSandbox`] and
/// [`LoadedJSSandbox`].
pub trait SnapshotTarget: private::Sealed + Sized {}

impl private::Sealed for JSSandbox {
    const KIND: SnapshotKind = SnapshotKind::JsSandbox;

    fn from_snapshot(
        inner: Sandbox,
        snapshot: &Snapshot<Self>,
        host_functions: HostFunctionManifest,
    ) -> Result<Self> {
        JSSandbox::from_snapshot(inner, snapshot, host_functions)
    }
}

impl SnapshotTarget for JSSandbox {}

impl private::Sealed for LoadedJSSandbox {
    const KIND: SnapshotKind = SnapshotKind::LoadedJsSandbox;

    fn from_snapshot(
        inner: Sandbox,
        snapshot: &Snapshot<Self>,
        host_functions: HostFunctionManifest,
    ) -> Result<Self> {
        LoadedJSSandbox::from_snapshot(inner, snapshot, host_functions)
    }
}

impl SnapshotTarget for LoadedJSSandbox {}

pub(crate) fn restore_target<T: SnapshotTarget>(
    inner: Sandbox,
    snapshot: &Snapshot<T>,
    host_functions: HostFunctionManifest,
) -> Result<T> {
    <T as private::Sealed>::from_snapshot(inner, snapshot, host_functions)
}

/// A persistent snapshot of a [`JSSandbox`] or [`LoadedJSSandbox`].
///
/// `T` is the sandbox type the snapshot restores to. Loading checks the
/// persisted kind once against `T`.
pub struct Snapshot<T> {
    inner: Arc<HyperlightSnapshot>,
    base: Option<Arc<HyperlightSnapshot>>,
    metadata: Arc<SnapshotMetadata>,
    target: PhantomData<fn() -> T>,
}

impl<T> Clone for Snapshot<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            base: self.base.clone(),
            metadata: Arc::clone(&self.metadata),
            target: PhantomData,
        }
    }
}

impl<T: SnapshotTarget> Snapshot<T> {
    /// Load a snapshot from an OCI image layout without checking blob digests.
    ///
    /// Fails if the snapshot captures a sandbox type other than `T`. The memory
    /// blob stays mapped while the snapshot or any sandbox built from it is
    /// alive. Overwriting, truncating, or deleting the layout's blobs during
    /// that time can corrupt guest memory and is undefined behavior.
    pub fn load(path: impl AsRef<Path>, reference: impl Into<OciReference>) -> Result<Self> {
        Self::load_inner(path.as_ref(), reference.into(), false)
    }

    /// Load a snapshot like [`Self::load`] and check every blob against its
    /// recorded sha256 digest.
    ///
    /// A digest check does not prove the bytes are authentic. Load only from a
    /// trusted source.
    pub fn checked_load(
        path: impl AsRef<Path>,
        reference: impl Into<OciReference>,
    ) -> Result<Self> {
        Self::load_inner(path.as_ref(), reference.into(), true)
    }

    fn load_inner(path: &Path, reference: OciReference, verified: bool) -> Result<Self> {
        let (inner, base, metadata) = load_parts(
            path,
            reference,
            verified,
            Some(<T as private::Sealed>::KIND),
        )?;
        Ok(Self::new(inner, base, metadata))
    }
}

type SnapshotParts = (
    Arc<HyperlightSnapshot>,
    Option<Arc<HyperlightSnapshot>>,
    Arc<SnapshotMetadata>,
);

fn load_parts(
    path: &Path,
    reference: OciReference,
    verified: bool,
    expected: Option<SnapshotKind>,
) -> Result<SnapshotParts> {
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
    if let Some(expected) = expected
        && metadata.kind() != expected
    {
        return Err(new_error!(
            "Snapshot captures a {:?}, not a {:?}",
            metadata.kind(),
            expected
        ));
    }

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

    Ok((inner, base, Arc::new(metadata)))
}

/// A snapshot of either sandbox type, for bindings that cannot name `T`.
#[doc(hidden)]
#[derive(Clone)]
pub enum AnySnapshot {
    /// A [`JSSandbox`] snapshot.
    JsSandbox(Snapshot<JSSandbox>),
    /// A [`LoadedJSSandbox`] snapshot.
    LoadedJsSandbox(Snapshot<LoadedJSSandbox>),
}

impl AnySnapshot {
    /// Load a snapshot of either type, like [`Snapshot::load`].
    pub fn load(path: impl AsRef<Path>, reference: impl Into<OciReference>) -> Result<Self> {
        Self::load_inner(path.as_ref(), reference.into(), false)
    }

    /// Load a snapshot of either type, like [`Snapshot::checked_load`].
    pub fn checked_load(
        path: impl AsRef<Path>,
        reference: impl Into<OciReference>,
    ) -> Result<Self> {
        Self::load_inner(path.as_ref(), reference.into(), true)
    }

    fn load_inner(path: &Path, reference: OciReference, verified: bool) -> Result<Self> {
        let (inner, base, metadata) = load_parts(path, reference, verified, None)?;
        Ok(match metadata.kind() {
            SnapshotKind::JsSandbox => Self::JsSandbox(Snapshot::new(inner, base, metadata)),
            SnapshotKind::LoadedJsSandbox => {
                Self::LoadedJsSandbox(Snapshot::new(inner, base, metadata))
            }
        })
    }

    /// Save the snapshot, like [`Snapshot::save`].
    pub fn save(&self, path: impl AsRef<Path>, tag: &OciTag) -> Result<OciDigest> {
        match self {
            Self::JsSandbox(snapshot) => snapshot.save(path, tag),
            Self::LoadedJsSandbox(snapshot) => snapshot.save(path, tag),
        }
    }
}

impl<T> Snapshot<T> {
    pub(crate) fn new(
        inner: Arc<HyperlightSnapshot>,
        base: Option<Arc<HyperlightSnapshot>>,
        metadata: Arc<SnapshotMetadata>,
    ) -> Self {
        Self {
            inner,
            base,
            metadata,
            target: PhantomData,
        }
    }

    /// Save the snapshot to an OCI image layout and return its manifest digest.
    ///
    /// A [`LoadedJSSandbox`] snapshot also saves its runtime-ready baseline under
    /// `<tag>.hljs-base`, which limits `tag` to 118 bytes.
    pub fn save(&self, path: impl AsRef<Path>, tag: &OciTag) -> Result<OciDigest> {
        let mut metadata = self.metadata.as_ref().clone();
        if let Some(base) = &self.base {
            // Hyperlight does not yet support OCI snapshot bundles, so save the
            // runtime-ready baseline separately and link it through metadata.
            // https://github.com/hyperlight-dev/hyperlight/issues/1865
            let base_tag = base_tag(tag)?;
            let base_digest = base.save(path.as_ref(), &base_tag)?;
            metadata.base_snapshot = Some(base_digest.to_string());
        }
        let snapshot = self.inner.with_metadata(METADATA_NAMESPACE, &metadata)?;

        snapshot.save(path, tag)
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
    fn host_function_validation_requires_every_recorded_function() {
        let mut metadata = metadata(SnapshotKind::LoadedJsSandbox);
        metadata.host_functions.insert(
            "host:database".to_owned(),
            vec!["execute".to_owned(), "query".to_owned()],
        );
        let available = |functions: &[&str]| {
            HostFunctionManifest::from([(
                "host:database".to_owned(),
                functions.iter().map(|name| (*name).to_owned()).collect(),
            )])
        };

        metadata
            .validate_host_functions(&available(&["execute", "extra", "query"]))
            .unwrap();
        let error = metadata
            .validate_host_functions(&available(&["query"]))
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("host function 'host:database.execute'"));
        let error = metadata
            .validate_host_functions(&HostFunctionManifest::new())
            .unwrap_err();
        assert!(error.to_string().contains("host module 'host:database'"));
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
