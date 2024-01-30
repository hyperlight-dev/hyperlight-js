#![cfg_attr(hyperlight, no_std)]
#![cfg_attr(hyperlight, no_main)]

#[cfg(hyperlight)]
include!("main/hyperlight.rs");

#[cfg(not(hyperlight))]
include!("main/native.rs");
