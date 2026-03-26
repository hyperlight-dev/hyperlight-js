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
#![cfg_attr(hyperlight, no_std)]
#![cfg_attr(hyperlight, no_main)]

// Provide the `init_native_modules` symbol required by the NativeModuleLoader.
// The upstream binary has no custom modules so this is empty.
// Extender binaries list their custom modules here instead.
// See: docs/extending-runtime.md
hyperlight_js_runtime::native_modules! {}

// Provide the `init_custom_globals` symbol required by JsRuntime::new().
// The upstream binary has no custom globals so this is empty.
// Extender binaries list their custom globals setup functions here instead.
hyperlight_js_runtime::custom_globals! {}

// The hyperlight guest entry point (hyperlight_main, guest_dispatch_function,
// etc.) is provided by the lib's `guest` module.
// The binary only needs to provide the native CLI entry point.

// Force the guest module to be linked into the final hyperlight binary.
// Without this, the linker may drop the guest module's object since nothing
// in main.rs references the guest entrypoints directly.
#[cfg(hyperlight)]
unsafe extern "C" {
    fn hyperlight_main();
}

#[cfg(hyperlight)]
#[used]
static _FORCE_GUEST_LINK: unsafe extern "C" fn() = hyperlight_main;

#[cfg(not(hyperlight))]
include!("main/native.rs");
