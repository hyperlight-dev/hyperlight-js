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
use rquickjs::{Ctx, Function, Module, Object};

pub fn setup(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let globals = ctx.globals();

    // Create console as a plain extensible Object (not the frozen module namespace).
    // This allows custom_globals! consumers to add methods (warn, error, info, debug)
    // before globals::freeze() locks it down.
    let console_mod: Object = Module::import(ctx, "console")?.finish()?;
    let console = Object::new(ctx.clone())?;
    console.set("log", console_mod.get::<_, Function>("log")?)?;
    globals.set("console", console)?;

    Ok(())
}
