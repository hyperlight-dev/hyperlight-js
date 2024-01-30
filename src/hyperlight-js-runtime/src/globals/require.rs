use rquickjs::object::Property;
use rquickjs::{Ctx, Function, Module, Object};

pub fn setup(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let globals = ctx.globals();

    // Setup `require` function.
    let require: Object = Module::import(ctx, "require")?.finish()?;
    globals.prop(
        "require",
        Property::from(require.get::<_, Function>("require")?),
    )?;

    Ok(())
}
