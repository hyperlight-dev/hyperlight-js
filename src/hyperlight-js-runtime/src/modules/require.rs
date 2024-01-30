use alloc::string::String;

use rquickjs::{Ctx, Module, Object, Result};

#[rquickjs::module(rename_vars = "camelCase", rename_types = "camelCase")]
#[allow(clippy::module_inception)]
pub mod require {
    use super::*;

    /// A thin wrapper around the so called "dynamic import" function `import()` that returns
    /// the module exports, or for modules with top-level await, it returns a promise that resolves
    /// to the module exports when the module is ready.
    #[rquickjs::function]
    pub fn require<'js>(ctx: Ctx<'js>, name: String) -> Result<Object<'js>> {
        let promise = Module::import(&ctx, name)?;
        match promise.finish::<Object<'js>>() {
            Ok(result) => Ok(result),
            Err(_) => {
                // The only error that finish can produce is `WouldBlock`, which simply
                // means that the promise can't be resolved yet.
                // In that case just return the promise.
                Ok(promise.into_inner())
            }
        }
    }

    // The default export is used when we do
    // ```js
    // import require from 'require'
    // ```
    // as opposed to a named export, which is used when we do
    // ```js
    // import { require } from 'require'
    // ```
    #[rquickjs::function]
    pub fn default<'js>(ctx: Ctx<'js>, name: String) -> Result<Object<'js>> {
        require(ctx, name)
    }
}
