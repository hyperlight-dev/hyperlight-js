use rquickjs::object::Property;
use rquickjs::{Ctx, Function, Module, Object};

pub fn setup(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let globals = ctx.globals();

    // Setup `print` function.
    let io: Object = Module::import(ctx, "io")?.finish()?;
    globals.prop("print", Property::from(io.get::<_, Function>("print")?))?;

    Ok(())
}
