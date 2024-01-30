use rquickjs::Ctx;

mod console;
mod print;
mod require;
mod string;

pub fn setup(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    string::setup(ctx)?;
    print::setup(ctx)?;
    console::setup(ctx)?;
    require::setup(ctx)?;
    Ok(())
}
