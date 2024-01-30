use alloc::string::{String, ToString as _};

use base64::engine::general_purpose::STANDARD_NO_PAD;
use base64::Engine as _;
use rquickjs::object::Property;
use rquickjs::{Ctx, Exception, Function, Object, String as JsString, Value};

use crate::utils::as_bytes;

#[rquickjs::function(rename = "bytesFrom")]
fn bytes_from<'js>(
    ctx: Ctx<'js>,
    data: Value<'js>,
    encoding: String,
) -> rquickjs::Result<JsString<'js>> {
    if encoding != "base64url" {
        return Err(Exception::throw_type(
            &ctx,
            "Unsupported encoding, only 'base64url' is supported",
        ));
    }

    let mut data = as_bytes(data)?;
    while data.last() == Some(&b'=') {
        // Remove padding characters
        data.pop();
    }
    match STANDARD_NO_PAD.decode(data) {
        Ok(bytes) => {
            let bytes = unsafe { str::from_utf8_unchecked(&bytes) };
            let string = JsString::from_str(ctx, bytes)?;
            Ok(string)
        }
        Err(e) => Err(Exception::throw_internal(&ctx, &e.to_string())),
    }
}

pub fn setup(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let globals = ctx.globals();

    // Setup `String.bytesFrom` function.
    let bytes_from = Function::new(ctx.clone(), bytes_from)?;
    let string: Object = globals.get("String")?;
    string.prop("bytesFrom", Property::from(bytes_from))?;
    Ok(())
}
