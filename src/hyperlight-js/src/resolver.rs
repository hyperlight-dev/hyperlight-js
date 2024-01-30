//! Module resolution and loading implementations.
//!
//! This module provides the core abstractions and implementations for loading
//! JavaScript modules into the sandbox environment.

use std::path::{Path, PathBuf};

pub use oxc_resolver::{FileMetadata, FileSystem, ResolveError};
use phf::Map;

/// File system implementation that uses embedded modules compiled into the binary.
///
/// This implementation stores all module contents in a compile-time perfect hash map,
/// eliminating any runtime file system access. This is the basic secure option for
/// module loading as it provides a completely closed set of available modules without
/// filesystem access.
///
/// # Example
///
/// ```no_run
/// use hyperlight_js::embed_modules;
///
/// let fs = embed_modules! {
///     "math.js" => "../tests/fixtures/math.js",
///     "strings.js" => "../tests/fixtures/strings.js",
/// };
///
/// ```
#[derive(Clone, Copy)]
pub struct FileSystemEmbedded {
    modules: &'static Map<&'static str, &'static str>,
}

impl FileSystemEmbedded {
    /// Create a new embedded file system with the given module map.
    ///
    /// See the `embed_modules!` macro for an easier way to create
    pub const fn new(modules: &'static Map<&'static str, &'static str>) -> Self {
        Self { modules }
    }

    /// Normalize a path for consistent lookups.
    fn normalize_path<'a>(&self, path: &'a Path) -> Option<std::borrow::Cow<'a, str>> {
        let s = path.to_str()?;

        if s.contains('\\') || s.starts_with("./") || s.starts_with('/') {
            Some(std::borrow::Cow::Owned(
                s.replace('\\', "/")
                    .trim_start_matches("./")
                    .trim_start_matches('/')
                    .to_string(),
            ))
        } else {
            Some(std::borrow::Cow::Borrowed(s))
        }
    }

    /// Check if a normalized path represents a directory by seeing if any
    /// embedded modules have this path as a prefix.
    fn is_directory(&self, normalized: &str) -> bool {
        if normalized.is_empty() {
            return !self.modules.is_empty();
        }

        let prefix = format!("{}/", normalized);
        self.modules.keys().any(|key| key.starts_with(&prefix))
    }
}

impl FileSystem for FileSystemEmbedded {
    fn new() -> Self {
        unreachable!("Use embed_modules! macro to create FileSystemEmbedded");
    }

    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        self.read_to_string(path).map(|s| s.into_bytes())
    }

    fn read_to_string(&self, path: &Path) -> std::io::Result<String> {
        let normalized = self.normalize_path(path).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid UTF-8 in path")
        })?;

        self.modules
            .get(&normalized)
            .map(|&content| content.to_string())
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Module '{}' not found", normalized),
                )
            })
    }

    fn metadata(&self, path: &Path) -> std::io::Result<FileMetadata> {
        let normalized = self.normalize_path(path).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid UTF-8 in path")
        })?;

        let is_file = self.modules.contains_key(normalized.as_ref());
        let is_dir = self.is_directory(normalized.as_ref());

        if !is_file && !is_dir {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Path '{}' not found", normalized),
            ));
        }

        Ok(FileMetadata::new(
            is_file, is_dir, false, /* is_symlink */
        ))
    }

    fn symlink_metadata(&self, path: &Path) -> std::io::Result<FileMetadata> {
        self.metadata(path)
    }

    fn read_link(&self, _path: &Path) -> Result<PathBuf, ResolveError> {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "symlinks are not supported in embedded file system",
        )
        .into())
    }

    fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf> {
        self.normalize_path(path)
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid UTF-8 in path")
            })
            .map(|v| PathBuf::from(v.into_owned()))
    }
}

/// Macro to create an embedded file system with compile-time included modules.
///
/// This macro simplifies the creation of an embedded file system by automatically
/// generating the `phf_map` and wrapping it in a `FileSystemEmbedded`.
///
/// # Example
///
/// ```text
/// embed_modules! {
///     "module_path" => "file_path",
///     "another_module" => "another_file",
///     ...
/// }
/// ```
///
/// ```no_run
/// use hyperlight_js::SandboxBuilder;
/// use hyperlight_js::embed_modules;
///
/// let fs = embed_modules! {
///     "math.js" => "../tests/fixtures/math.js",
///     "strings.js" => "../tests/fixtures/strings.js",
/// };
///
/// let proto_js_sandbox = SandboxBuilder::new().build().unwrap();
/// let sandbox = proto_js_sandbox
///     .set_module_loader(fs)
///     .unwrap()
///     .load_runtime()
///     .unwrap();
/// ```
///
/// // With inline content:
/// let fs = embed_modules! {
///     "test.js" => @inline "console.log('test');",
/// };
///
/// # Notes
///
/// * File paths are relative to the current file
#[macro_export]
macro_rules! embed_modules {
    // Match file: prefix
    ($($key:expr => $file:expr),* $(,)?) => {{
        use $crate::FileSystemEmbedded;
        use ::phf::{phf_map, Map};

        static EMBEDDED_MODULES: Map<&'static str, &'static str> = phf_map! {
            $(
                $key => include_str!($file),
            )*
        };

        FileSystemEmbedded::new(&EMBEDDED_MODULES)
    }};

    // Match @inline prefix
    ($($key:expr => @inline $content:expr),* $(,)?) => {{
        use $crate::FileSystemEmbedded;
        use ::phf::{phf_map, Map};

        static EMBEDDED_MODULES: Map<&'static str, &'static str> = phf_map! {
            $(
                $key => $content,
            )*
        };

        FileSystemEmbedded::new(&EMBEDDED_MODULES)
    }};
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn test_file_read() {
        let fs = embed_modules! {
            "test.js" => @inline "console.log('hello');",
        };

        let content = fs.read_to_string(Path::new("test.js")).unwrap();
        assert_eq!(content, "console.log('hello');");
    }

    #[test]
    fn test_directory_detection() {
        let fs = embed_modules! {
            "foo/bar.js" => @inline "content",
        };

        let metadata = fs.metadata(Path::new("foo")).unwrap();
        assert!(metadata.is_dir());
        assert!(!metadata.is_file());
    }

    #[test]
    fn test_file_metadata() {
        let fs = embed_modules! {
            "test.js" => @inline "content",
        };

        let metadata = fs.metadata(Path::new("test.js")).unwrap();
        assert!(metadata.is_file());
        assert!(!metadata.is_dir());
    }

    #[test]
    fn test_prefix_collision() {
        let fs = embed_modules! {
            "foo.js" => @inline "content1",
            "foobar.js" => @inline "content2",
        };

        assert!(fs.metadata(Path::new("foo")).is_err());
        assert!(fs.metadata(Path::new("foo.js")).unwrap().is_file());
    }

    #[test]
    fn test_not_found() {
        let fs = embed_modules! {
            "exists.js" => @inline "content",
        };

        let result = fs.read_to_string(Path::new("missing.js"));
        assert!(result.is_err());
    }
}
