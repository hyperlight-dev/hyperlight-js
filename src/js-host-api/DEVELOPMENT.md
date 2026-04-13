# Hyperlight JS Host API — Development Guide

Development, build, and publishing instructions for contributors.

## Requirements

- **Node.js** >= 18
- **Rust** toolchain (see `rust-toolchain.toml` in repository root)
- **just** (build automation) — install via `cargo install just` or your package manager

## Building from Source

### Build Commands

```bash
# Install dependencies
npm install

# Release builds (optimized)
npm run build

# Debug builds (with symbols)
npm run build:debug

# Run tests
npm test
```

### Using Just (Build Automation)

From the repository root:

```bash
# Build js-host-api
just build-js-host-api release

# Build with debug symbols
just build-js-host-api debug

# Run js-host-api examples
just run-js-host-api-examples release

# Run js-host-api tests
just test-js-host-api release

# Build and test everything (all runtimes and targets)
just build-all
just test-all release
```

## Publishing to npm

The package is published to npmjs.com as `@hyperlight-dev/js-host-api` with platform-specific binary packages using [npm trusted publishing (OIDC)](https://docs.npmjs.com/trusted-publishers).

> **Note:** You cannot `npm publish` from your local machine. Publishing is handled by CI via the `CreateRelease` workflow, which uses OIDC for authentication. Trusted publishing must be configured on npmjs.com — see [docs/release.md](../../docs/release.md) for full setup and release instructions.

**For release instructions, see [docs/release.md](../../docs/release.md).**

### Package Structure

The npm release consists of the following packages:

| Package | Description |
|---------|-------------|
| `@hyperlight-dev/js-host-api` | Main package (installs correct binary automatically) |
| `@hyperlight-dev/js-host-api-linux-x64-gnu` | Linux x86_64 (glibc) native binary |
| `@hyperlight-dev/js-host-api-linux-x64-musl` | Linux x86_64 (musl/Alpine) native binary |
| `@hyperlight-dev/js-host-api-win32-x64-msvc` | Windows x86_64 native binary |

### How Platform Selection Works

This project uses the [napi-rs](https://napi.rs/docs/deep-dive/release#3-the-native-addon-for-different-platforms-is-distributed-through-different-npm-packages) approach for distributing native addons across platforms. Each platform-specific binary is published as a separate npm package and listed as an `optionalDependency` of the main package.

**At install time:** npm uses the `os`, `cpu`, and `libc` fields in each platform sub-package's `package.json` to determine which optional dependency to install. Packages that don't match the user's platform are silently skipped. The main package itself does **not** have `os`/`cpu` fields because it contains only JavaScript — restricting it would prevent installation on unsupported platforms even for type-checking or development purposes.

**At runtime:** The napi-rs generated `index.js` detects the platform (including glibc vs musl on Linux) and loads the correct `.node` binary.
