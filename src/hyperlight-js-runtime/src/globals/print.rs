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
use rquickjs::object::Property;
use rquickjs::{Ctx, Function, Module, Object};

pub fn setup(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let globals = ctx.globals();

    // Setup `print` function.
    let io: Object = Module::import(ctx, "io")?.finish()?;
    globals.prop("print", Property::from(io.get::<_, Function>("print")?))?;

    Ok(())
}
