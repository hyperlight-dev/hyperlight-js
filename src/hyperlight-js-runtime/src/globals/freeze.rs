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
use rquickjs::Ctx;

/// Freeze built-in globals so handler code cannot tamper with them.
///
/// Called AFTER custom_globals! so extender crates can modify/extend
/// globals first (e.g. adding console.warn/error/info/debug).
///
/// Frozen: console (Object.freeze + non-writable/non-configurable binding),
///         print (non-writable/non-configurable binding).
/// Already frozen: require (non-configurable from setup),
///                 String.bytesFrom (on frozen String constructor).
pub fn setup(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(
        r#"
        if (typeof globalThis.console === 'object' || typeof globalThis.console === 'function') {
            Object.freeze(globalThis.console);
        }
        if ('console' in globalThis) {
            Object.defineProperty(globalThis, 'console', {
                writable: false,
                configurable: false
            });
        }
        if (typeof globalThis.print === 'function') {
            Object.defineProperty(globalThis, 'print', {
                writable: false,
                configurable: false
            });
        }
        "#,
    )?;
    Ok(())
}
