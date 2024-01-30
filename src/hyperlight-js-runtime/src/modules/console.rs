use alloc::string::String;

use rquickjs::prelude::Rest;
use rquickjs::Coerced;

use super::io::io::print;

#[rquickjs::module(rename_vars = "camelCase", rename_types = "camelCase")]
#[allow(clippy::module_inception)]
pub mod console {
    use super::*;

    #[rquickjs::function]
    pub fn log(txt: Rest<Coerced<String>>) -> rquickjs::Result<()> {
        let mut txt = txt
            .into_inner()
            .into_iter()
            .map(|mut c| {
                c.0.push(' ');
                c.0
            })
            .collect::<String>();
        txt.pop(); // remove the last space
        txt.push('\n'); // add a newline at the end
        print(txt);
        Ok(())
    }
}
