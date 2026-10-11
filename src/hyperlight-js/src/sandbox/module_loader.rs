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

use std::path::PathBuf;

use hyperlight_host::{
    new_error, Result, SandboxBuilder as HyperlightSandboxBuilder, UninitializedSandbox,
};
use oxc_resolver::{ResolveOptions, ResolverGeneric};

pub(super) const RESOLVE_MODULE_HOST_FUNCTION: &str = "ResolveModule";
pub(super) const LOAD_MODULE_HOST_FUNCTION: &str = "LoadModule";

type ResolveModule = Box<dyn Fn(String, String) -> Result<String> + Send + Sync>;
type LoadModule = Box<dyn Fn(String) -> Result<String> + Send + Sync>;

pub(super) struct ExternalModuleLoader {
    resolve: ResolveModule,
    load: LoadModule,
}

impl ExternalModuleLoader {
    pub(super) fn new<Fs: crate::resolver::FileSystem + Clone + 'static>(file_system: Fs) -> Self {
        let resolver = ResolverGeneric::new_with_file_system(
            file_system.clone(),
            ResolveOptions {
                extensions: vec![".js".into(), ".mjs".into()],
                condition_names: vec!["import".into(), "module".into()],
                ..Default::default()
            },
        );
        let resolve = Box::new(move |base: String, specifier: String| {
            let resolved = resolver.resolve(&base, &specifier).map_err(|error| {
                new_error!(
                    "Failed to resolve module '{}' from '{}': {:?}",
                    specifier,
                    base,
                    error
                )
            })?;
            Ok(resolved.path().to_string_lossy().to_string())
        });
        let load = Box::new(move |path: String| {
            file_system
                .read_to_string(&PathBuf::from(&path))
                .map_err(|error| new_error!("Failed to read module '{}': {}", path, error))
        });
        Self { resolve, load }
    }

    pub(super) fn register(self, sandbox: &mut UninitializedSandbox) -> Result<()> {
        sandbox.register(
            RESOLVE_MODULE_HOST_FUNCTION,
            move |base: String, specifier: String| (self.resolve)(base, specifier),
        )?;
        sandbox.register(LOAD_MODULE_HOST_FUNCTION, move |path: String| {
            (self.load)(path)
        })?;
        Ok(())
    }

    pub(super) fn apply(self, builder: HyperlightSandboxBuilder) -> HyperlightSandboxBuilder {
        let builder = builder.host_function(
            RESOLVE_MODULE_HOST_FUNCTION,
            move |base: String, specifier: String| (self.resolve)(base, specifier),
        );
        builder.host_function(LOAD_MODULE_HOST_FUNCTION, move |path: String| {
            (self.load)(path)
        })
    }
}
