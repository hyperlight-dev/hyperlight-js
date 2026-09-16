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

/*
 * Shim over the guest sysroot's <math.h>.
 *
 * The guest sysroot is newlib. Newlib defines the C99 floating-point
 * classification macros with `__builtin_*` only for GCC — clang is explicitly
 * excluded (see the `!defined(__clang__)` guard around its `__builtin_fpclassify`
 * block), so under clang it falls back to a sizeof-dispatch macro of this shape:
 *
 *     #define isnan(__x) ((sizeof(__x) == sizeof(float))  ? __isnanf(__x)
 *                       : (sizeof(__x) == sizeof(double)) ? __isnand((double)(__x))
 *                                                         : __isnanl((long double)(__x)))
 *
 * The selection happens at compile time, but *every* branch is still type-checked
 * and code-generated, including the `(long double)` cast of a `double` argument.
 *
 * On x86_64 that cast is free: `long double` is the 80-bit x87 type and the
 * extension is a hardware instruction. On aarch64 `long double` is IEEE binary128,
 * so the cast becomes a call to the soft-float builtin `__extenddftf2` (and
 * `__extendsftf2` for floats), which the guest does not link — the guest links
 * Rust's `compiler_builtins`, whose f128 routines are not enabled in the
 * precompiled bare-metal sysroot. QuickJS calls `isnan`/`isfinite` heavily, so
 * this shows up as a wall of undefined-symbol errors at link time.
 *
 * We are compiled by clang (cargo-hyperlight always drives a clang C compiler),
 * and clang's `__builtin_*` classification builtins are type-generic and lower to
 * plain FP instructions with no libcall, on both architectures. So redefine the
 * macros to exactly what newlib itself uses on its GCC path.
 *
 * This file is reachable because the guest build passes
 * `-I <hyperlight-js-runtime>/include` ahead of the `-isystem` sysroot, so this
 * header wins the lookup for `<math.h>` and `#include_next` then pulls in the
 * real newlib header behind it.
 */

#ifndef HYPERLIGHT_JS_MATH_SHIM_H
#define HYPERLIGHT_JS_MATH_SHIM_H

#include_next <math.h>

#ifdef __clang__

#undef fpclassify
#undef isfinite
#undef isinf
#undef isnan
#undef isnormal
#undef signbit

#define fpclassify(__x)                                                        \
    (__builtin_fpclassify(FP_NAN, FP_INFINITE, FP_NORMAL, FP_SUBNORMAL,        \
                          FP_ZERO, __x))
#define isfinite(__x) (__builtin_isfinite(__x))
#define isinf(__x) (__builtin_isinf_sign(__x))
#define isnan(__x) (__builtin_isnan(__x))
#define isnormal(__x) (__builtin_isnormal(__x))
#define signbit(__x) (__builtin_signbit(__x))

#endif /* __clang__ */

#endif /* HYPERLIGHT_JS_MATH_SHIM_H */
