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
