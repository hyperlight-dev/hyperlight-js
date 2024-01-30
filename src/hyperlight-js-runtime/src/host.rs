use alloc::string::String;

use anyhow::Result;

/// A trait representing the host environment for the JS runtime.
/// In hyperlight this would represent the host function calls that
/// the runtime needs.
pub trait Host: Send + Sync {
    /// Resolve a module name to a module specifier (usually a path).
    /// The base is the specifier of the module that is importing the module.
    fn resolve_module(&self, base: String, name: String) -> Result<String>;

    /// Obtain the module source code for a given module specifier.
    fn load_module(&self, name: String) -> Result<String>;
}
