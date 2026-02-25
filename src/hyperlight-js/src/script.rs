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
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::{new_error, Result};

/// Represents a JavaScript immutable handler script with metadata about its source location.
/// The source location metadata is required to resolve relative locations when the script imports
/// other modules using relative paths.
#[derive(Debug, Clone)]
pub struct Script {
    /// The script content
    content: Arc<str>,
    /// base path for resolving module imports
    base_path: Option<PathBuf>,
}

impl Script {
    /// Create a script from a string with no base path for module resolution
    pub fn from_content(content: impl Into<String>) -> Self {
        // TODO(tandr): Consider validating the script content using oxc_parser or similar
        Self {
            content: Arc::from(content.into()),
            base_path: None,
        }
    }

    /// Create a script by reading from a file
    ///
    /// The base path is automatically set to the directory containing the file
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();

        let content = std::fs::read_to_string(path)
            .map_err(|e| new_error!("Failed to read script from '{}': {}", path.display(), e))?;

        let base_path = path.parent().map(|p| p.to_path_buf());
        Ok(Self {
            content: Arc::from(content),
            base_path,
        })
    }

    /// Set a virtual base path for module resolution.
    pub fn with_virtual_base(mut self, path: impl AsRef<str>) -> Self {
        self.base_path = Some(PathBuf::from(path.as_ref()));
        self
    }

    /// Get the script content
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Get the base path for module resolution, if any
    pub fn base_path(&self) -> Option<&Path> {
        self.base_path.as_deref()
    }
}

impl From<String> for Script {
    fn from(content: String) -> Self {
        Self::from_content(content)
    }
}

impl From<&str> for Script {
    fn from(content: &str) -> Self {
        Self::from_content(content)
    }
}

impl TryFrom<&Path> for Script {
    type Error = hyperlight_host::HyperlightError;
    fn try_from(path: &Path) -> Result<Self> {
        Self::from_file(path)
    }
}
