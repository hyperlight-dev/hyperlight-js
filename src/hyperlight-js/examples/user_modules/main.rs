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

//! User Modules example — register reusable ES modules that handlers can share.
//!
//! Demonstrates:
//! - Registering user modules with `add_module()`
//! - Handlers importing a shared module via `import { ... } from 'user:<name>'`
//! - **Cross-handler mutable state sharing** via a shared module
//! - Inter-module dependencies
//! - Custom namespaces
//!
//! Run with:
//! ```bash
//! cargo run --example user_modules
//! ```

#![allow(clippy::disallowed_macros)]

use anyhow::Result;
use hyperlight_js::{SandboxBuilder, Script};

fn main() -> Result<()> {
    println!("=== Hyperlight JS User Modules ===\n");

    // ── Build sandbox ────────────────────────────────────────────────
    println!("1. Creating sandbox...");
    let proto = SandboxBuilder::new().build()?;
    let mut sandbox = proto.load_runtime()?;
    println!("   ✓ Sandbox created\n");

    // ── Part 1: Shared pure-function module ──────────────────────────
    println!("2. Registering user modules...");

    // A constants module — other modules can import from it
    sandbox.add_module(
        "constants",
        Script::from_content(
            r#"
            export const PI = 3.14159;
            export const E = 2.71828;
            "#,
        ),
    )?;

    // A geometry module that depends on the constants module
    sandbox.add_module(
        "geometry",
        Script::from_content(
            r#"
            import { PI } from 'user:constants';
            export function circleArea(radius) { return PI * radius * radius; }
            export function circleCircumference(radius) { return 2 * PI * radius; }
            "#,
        ),
    )?;

    // A string utils module with a custom namespace
    sandbox.add_module_ns(
        "strings",
        Script::from_content(
            r#"
            export function capitalize(s) {
                return s.charAt(0).toUpperCase() + s.slice(1);
            }
            export function reverse(s) {
                return s.split('').reverse().join('');
            }
            "#,
        ),
        "mylib",
    )?;

    println!("   ✓ 3 pure-function modules registered\n");

    // ── Part 2: Shared mutable state module ──────────────────────────
    //
    // This is the key pattern — a module with mutable state that
    // multiple handlers can read and write. ESM singleton semantics
    // guarantee all importers see the same module instance.

    // A counter module with mutable module-level state
    sandbox.add_module(
        "counter",
        Script::from_content(
            r#"
            let count = 0;
            export function increment() { return ++count; }
            export function getCount() { return count; }
            "#,
        ),
    )?;

    // A key-value store module for richer shared state
    sandbox.add_module(
        "store",
        Script::from_content(
            r#"
            const data = new Map();
            export function set(key, value) { data.set(key, value); }
            export function get(key) { return data.get(key); }
            export function entries() { return Object.fromEntries(data); }
            "#,
        ),
    )?;

    println!("   ✓ 2 shared-state modules registered\n");

    // ── Register handlers ────────────────────────────────────────────
    println!("3. Adding handlers...");

    // Handler: geometry (uses pure functions via inter-module deps)
    sandbox.add_handler(
        "circle",
        Script::from_content(
            r#"
            import { circleArea, circleCircumference } from 'user:geometry';
            export function handler(event) {
                return {
                    radius: event.radius,
                    area: circleArea(event.radius),
                    circumference: circleCircumference(event.radius),
                };
            }
            "#,
        ),
    )?;

    // Handler: string processing (custom namespace)
    sandbox.add_handler(
        "strings",
        Script::from_content(
            r#"
            import { capitalize, reverse } from 'mylib:strings';
            export function handler(event) {
                return {
                    original: event.text,
                    capitalized: capitalize(event.text),
                    reversed: reverse(event.text),
                };
            }
            "#,
        ),
    )?;

    // Handler: counter writer — mutates the shared counter module
    sandbox.add_handler(
        "counter_writer",
        Script::from_content(
            r#"
            import { increment } from 'user:counter';
            export function handler(event) {
                event.count = increment();
                return event;
            }
            "#,
        ),
    )?;

    // Handler: counter reader — reads state WITHOUT mutating it
    sandbox.add_handler(
        "counter_reader",
        Script::from_content(
            r#"
            import { getCount } from 'user:counter';
            export function handler(event) {
                event.count = getCount();
                return event;
            }
            "#,
        ),
    )?;

    // Handler: store writer — writes key-value pairs to the shared store
    sandbox.add_handler(
        "store_put",
        Script::from_content(
            r#"
            import { set } from 'user:store';
            export function handler(event) {
                set(event.key, event.value);
                return { ok: true };
            }
            "#,
        ),
    )?;

    // Handler: store reader — reads back from the shared store
    sandbox.add_handler(
        "store_get",
        Script::from_content(
            r#"
            import { get, entries } from 'user:store';
            export function handler(event) {
                return {
                    value: get(event.key),
                    all: entries(),
                };
            }
            "#,
        ),
    )?;

    let mut loaded = sandbox.get_loaded_sandbox()?;
    println!("   ✓ 6 handlers loaded\n");

    // ── Call pure-function handlers ───────────────────────────────────
    println!("4. Calling pure-function handlers...\n");

    let circle = loaded.handle_event("circle", r#"{"radius": 5}"#.to_string(), None)?;
    println!("   Circle (radius=5): {circle}");

    let strings = loaded.handle_event("strings", r#"{"text": "hyperlight"}"#.to_string(), None)?;
    println!("   Strings (\"hyperlight\"): {strings}\n");

    // ── Demonstrate cross-handler shared mutable state ────────────────
    println!("5. Cross-handler shared mutable state (counter)...\n");

    let r1 = loaded.handle_event("counter_writer", "{}".to_string(), None)?;
    println!("   Writer call 1 → {r1}");

    let r2 = loaded.handle_event("counter_reader", "{}".to_string(), None)?;
    println!("   Reader sees  → {r2}  (should match writer's count)");

    let r3 = loaded.handle_event("counter_writer", "{}".to_string(), None)?;
    println!("   Writer call 2 → {r3}");

    let r4 = loaded.handle_event("counter_reader", "{}".to_string(), None)?;
    println!("   Reader sees  → {r4}  (should match writer's count)\n");

    // ── Demonstrate cross-handler shared key-value store ──────────────
    println!("6. Cross-handler shared mutable state (key-value store)...\n");

    loaded.handle_event(
        "store_put",
        r#"{"key": "name", "value": "Hyperlight"}"#.to_string(),
        None,
    )?;
    println!("   Put: name = \"Hyperlight\"");

    loaded.handle_event(
        "store_put",
        r#"{"key": "year", "value": 1985}"#.to_string(),
        None,
    )?;
    println!("   Put: year = 1985");

    let store_result = loaded.handle_event("store_get", r#"{"key": "name"}"#.to_string(), None)?;
    println!("   Get(name): {store_result}");

    let store_all = loaded.handle_event("store_get", r#"{"key": "year"}"#.to_string(), None)?;
    println!("   Get(year): {store_all}");

    println!("\n✅ User modules example complete! — \"Life moves pretty fast. If you don't stop and share state once in a while, you could miss it.\"");
    Ok(())
}
