use rquickjs::object::Property;
use rquickjs::{Ctx, Module, Object};

pub fn setup(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let globals = ctx.globals();

    // Setup `console`.
    let console: Object = Module::import(ctx, "console")?.finish()?;
    globals.prop("console", Property::from(console))?;

    Ok(())
}
