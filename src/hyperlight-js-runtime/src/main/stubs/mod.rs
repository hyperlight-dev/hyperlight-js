/*
Copyright 2026 The Hyperlight Authors.

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
//! Some of the libc functions that rquickjs requires are not implemented in
//! the libc provided by the hyperlight runtime, so we provide our own implementations
//! here. We also re-export the generated bindings for the rest of the libc functions.

mod clock;
mod io;
mod localtime;
mod srand;
