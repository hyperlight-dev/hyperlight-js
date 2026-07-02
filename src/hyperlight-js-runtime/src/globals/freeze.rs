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
use rquickjs::{Ctx, Function, Object, Value};

/// Freeze built-in globals so handler code cannot tamper with them.
///
/// Called AFTER custom_globals! so extender crates can modify/extend
/// globals first (e.g. adding console.warn/error/info/debug).
///
/// Frozen: console (Object.freeze + non-writable/non-configurable binding),
///         print (non-writable/non-configurable binding).
/// Already frozen: require (non-configurable from setup),
///                 String.bytesFrom (on frozen String constructor).
///
/// This mirrors what `Object.freeze` / `Object.defineProperty` would do from
/// JavaScript, but drives those built-ins directly through the rquickjs API so
/// there is no embedded script string to parse.
pub fn setup(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let globals = ctx.globals();

    // Look up the standard `Object.freeze` / `Object.defineProperty` helpers
    // once, then reuse them for each global below.
    let object_ctor: Object = globals.get("Object")?;
    let freeze: Function = object_ctor.get("freeze")?;
    let define_property: Function = object_ctor.get("defineProperty")?;

    // 1. Freeze the `console` object itself (equivalent to
    //    `Object.freeze(console)`) so its methods cannot be swapped out. Only
    //    applies when `console` is actually an object.
    if let Some(console) = globals.get::<_, Option<Object>>("console")? {
        freeze.call::<_, ()>((console,))?;
    }

    // 2. Lock the `console` global *binding* (non-writable, non-configurable)
    //    so handler code cannot replace `globalThis.console` wholesale.
    if globals.contains_key("console")? {
        lock_binding(&define_property, &globals, "console")?;
    }

    // 3. Lock the `print` global binding, but only when it is a function
    //    (matches the previous `typeof print === 'function'` guard).
    if globals.get::<_, Value>("print")?.is_function() {
        lock_binding(&define_property, &globals, "print")?;
    }

    Ok(())
}

/// Redefine `globals[name]` as a non-writable, non-configurable property while
/// preserving its current value — the Rust equivalent of
/// `Object.defineProperty(globalThis, name, { writable: false, configurable: false })`.
fn lock_binding<'js>(
    define_property: &Function<'js>,
    globals: &Object<'js>,
    name: &str,
) -> rquickjs::Result<()> {
    let descriptor = Object::new(globals.ctx().clone())?;
    descriptor.set("writable", false)?;
    descriptor.set("configurable", false)?;
    define_property.call::<_, ()>((globals.clone(), name, descriptor))?;
    Ok(())
}
