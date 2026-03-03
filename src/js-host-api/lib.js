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
// ── Hyperlight JS Host API — Error enrichment wrapper ────────────────
//
// This module re-exports the native napi-rs binding from index.js and
// enriches thrown errors with structured `error.code` values.
//
// ## Why this exists
//
// napi-rs only implements `ToNapiValue` for `Result<T, Error<Status>>`,
// not for custom error status types. Since all our methods are async
// (using `spawn_blocking`), we can't use `Error<CustomStatus>` — the
// async return path requires `ToNapiValue` on the Result.
//
// Our Rust code embeds domain-specific error codes as `[ERR_*]` prefixes
// in error messages. This wrapper parses those prefixes and promotes
// them to `error.code`, giving consumers idiomatic Node.js error handling:
//
//   catch (e) {
//     if (e.code === 'ERR_POISONED') { await loaded.restore(snapshot); }
//   }
//
// ## What would fix this upstream
//
// napi-rs would need to make `ToNapiValue` generic over the error status:
//
//   impl<T, S: AsRef<str>> ToNapiValue for Result<T, Error<S>> { ... }
//
// instead of the current:
//
//   impl<T> ToNapiValue for Result<T> { ... }  // Result<T> = Result<T, Error<Status>>
//
// See: napi-rs/napi-rs — crates/napi/src/bindgen_runtime/js_values.rs
// When that lands, this wrapper can be removed and the Rust side can use
// `Error<ErrorCode>` directly on async functions.
//
// ─────────────────────────────────────────────────────────────────────

'use strict';

const native = require('./index.js');

// ── Error enrichment ─────────────────────────────────────────────────

// Matches `[ERR_CODE]` prefix at the start of error messages.
// The Rust side formats errors as: `[ERR_POISONED] sandbox is poisoned`
const ERROR_CODE_RE = /^\[(ERR_\w+)\]\s*/;

/**
 * Extracts a `[ERR_*]` prefix from an error's message and promotes it
 * to `error.code`. Strips the prefix from the message for cleanliness.
 *
 * @param {Error} err — the error to enrich (mutated in place)
 * @returns {Error} the same error, for chaining
 */
function enrichError(err) {
    if (err instanceof Error) {
        const match = err.message.match(ERROR_CODE_RE);
        if (match) {
            err.code = match[1];
            err.message = err.message.slice(match[0].length);
        }
    }
    return err;
}

/**
 * Wraps an async method so that rejected promises have enriched errors.
 *
 * @param {Function} fn — the original async method
 * @returns {Function} a wrapper that catches and enriches errors
 */
function wrapAsync(fn) {
    return async function (...args) {
        try {
            return await fn.apply(this, args);
        } catch (err) {
            throw enrichError(err);
        }
    };
}

/**
 * Wraps a sync method so that thrown errors have enriched codes.
 *
 * @param {Function} fn — the original sync method
 * @returns {Function} a wrapper that catches and enriches errors
 */
function wrapSync(fn) {
    return function (...args) {
        try {
            return fn.apply(this, args);
        } catch (err) {
            throw enrichError(err);
        }
    };
}

// ── Prototype patching ───────────────────────────────────────────────
//
// We patch the native class prototypes when this module is loaded so that
// all consumers in the same process (including code that later requires
// index.js directly) get enriched errors. The native binding module is
// cached by require(), so prototypes are patched once per process, after
// this module has been required at least once.

const { LoadedJSSandbox, JSSandbox, ProtoJSSandbox, SandboxBuilder, HostModule } = native;

/**
 * Wrap a getter so that thrown errors have enriched codes.
 *
 * @param {Function} cls    — the class whose prototype to patch
 * @param {string}   prop   — the property name with a getter
 */
function wrapGetter(cls, prop) {
    const desc = Object.getOwnPropertyDescriptor(cls.prototype, prop);
    if (!desc || !desc.get) {
        throw new Error(`Cannot wrap missing getter: ${cls.name}.${prop}`);
    }
    const origGet = desc.get;
    Object.defineProperty(cls.prototype, prop, {
        ...desc,
        get() {
            try {
                return origGet.call(this);
            } catch (err) {
                throw enrichError(err);
            }
        },
    });
}

// LoadedJSSandbox — async methods
// Note: `poisoned` (AtomicBool read) and `interruptHandle` (Arc clone)
// are infallible getters — no wrapping needed.
for (const method of ['callHandler', 'unload', 'snapshot', 'restore']) {
    const orig = LoadedJSSandbox.prototype[method];
    if (!orig) throw new Error(`Cannot wrap missing method: LoadedJSSandbox.${method}`);
    LoadedJSSandbox.prototype[method] = wrapAsync(orig);
}

// JSSandbox — async + sync methods + getters
JSSandbox.prototype.getLoadedSandbox = wrapAsync(JSSandbox.prototype.getLoadedSandbox);

for (const method of ['addHandler', 'removeHandler', 'clearHandlers']) {
    const orig = JSSandbox.prototype[method];
    if (!orig) throw new Error(`Cannot wrap missing method: JSSandbox.${method}`);
    JSSandbox.prototype[method] = wrapSync(orig);
}
wrapGetter(JSSandbox, 'poisoned');

// ProtoJSSandbox — async + sync methods
ProtoJSSandbox.prototype.loadRuntime = wrapAsync(ProtoJSSandbox.prototype.loadRuntime);

// hostModule() is sync — just wrap for error enrichment
ProtoJSSandbox.prototype.hostModule = wrapSync(ProtoJSSandbox.prototype.hostModule);

// ProtoJSSandbox — register() handle errors and wraps callback to return Promise
{
    const origRegister = ProtoJSSandbox.prototype.register;
    ProtoJSSandbox.prototype.register = wrapSync(function (moduleName, functionName, callback) {
        // the rust code expects the host function to return a Promise, so we wrap the callback result in Promise.resolve().then(..) to allow sync functions as well
        // note that Promise.resolve(callback(...args)) would not work because if callback throws that would not return a rejected promise, it would just throw before returning the promise.
        return origRegister.call(this, moduleName, functionName, (...args) =>
            Promise.resolve().then(() => callback(...args))
        );
    });
}

// HostModule — register()
{
    const origRegister = HostModule.prototype.register;
    if (!origRegister) throw new Error('Cannot wrap missing method: HostModule.register');
    HostModule.prototype.register = wrapSync(function (name, callback) {
        // the rust code expects the host function to return a Promise, so we wrap the callback result in Promise.resolve().then(..) to allow sync functions as well
        // note that Promise.resolve(callback(...args)) would not work because if callback throws that would not return a rejected promise, it would just throw before returning the promise.
        return origRegister.call(this, name, (...args) =>
            Promise.resolve().then(() => callback(...args))
        );
    });
}

// SandboxBuilder — async build + sync setters
SandboxBuilder.prototype.build = wrapAsync(SandboxBuilder.prototype.build);

for (const method of [
    'setHeapSize',
    'setScratchSize',
    'setInputBufferSize',
    'setOutputBufferSize',
]) {
    const orig = SandboxBuilder.prototype[method];
    if (!orig) throw new Error(`Cannot wrap missing method: SandboxBuilder.${method}`);
    SandboxBuilder.prototype[method] = wrapSync(orig);
}

// ── Re-export ────────────────────────────────────────────────────────

module.exports = native;
