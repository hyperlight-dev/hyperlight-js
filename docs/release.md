# Create a new hyperlight-js release

This document details the process of releasing a new version of hyperlight-js to [crates.io](https://crates.io/) and [npmjs.com](https://www.npmjs.com/). It's intended to be used as a checklist for the developer doing the release. 

## Update versions

The first step in the release process is to bump the version numbers — the Rust crates **and** the npm packages — keeping them all in sync.

Do this with the `just set-version` recipe. **Always use this instead of bumping by hand** (`cargo set-version`, `npm version`, editing `package.json`): piecemeal bumps are exactly what leaves a lockfile stale and breaks CI mid-release. In one step it updates:

- every workspace crate's `version` and the root `Cargo.lock`,
- the excluded `extended_runtime` fixture's own `Cargo.lock` (a bare `cargo set-version` can't reach it, and a stale one fails the `native_modules --locked` build),
- the npm main package, the three platform packages, and their `optionalDependencies`,
- `src/js-host-api/package-lock.json` (a stale one fails `npm ci` in the publish job).

It uses `cargo set-version` from the `cargo-edit` crate under the hood, so install that first:

```console
cargo install cargo-edit
```

Then bump everything:

```console
just set-version 0.18.0
```

We keep the version number consistent across all crates and npm packages in the repository.

Create a PR with these changes and merge it into the `main` branch.

> **Note:** The `CreateRelease` workflow *also* sets the npm packages to the tag's version at publish time (via `npm version`), so the published artifacts always match the tag regardless. Bumping them in the repo with `just set-version` is what keeps `npm ci` from failing *during* the release — don't skip it.

## Create a tag

When the `main` branch has reached a state in which you want to release a new Cargo version, you should create a tag. Although you can do this from the GitHub releases page, we currently recommend doing the tag from the command line. Do so with the following commands:

```bash
git tag -a v0.18.0 -m "A brief description of the release"
git push origin v0.18.0 # if you've named your git remote for the hyperlight-dev/hyperlight-js repo differently, change 'origin' to your remote name
```

>Note: we'll use `v0.18.0` as the version for the above and all subsequent instructions. You should replace this with the version you're releasing. Make sure your version follows [SemVer](https://semver.org) conventions as closely as possible, and is prefixed with a `v` character. *In particular do not use a patch version unless you are patching an issue in a release branch, releases from main should always be minor or major versions*.
If you are creating a patch release see the instructions [here](#patching-a-release).

## What happens when you push the tag

Pushing a `vX.Y.Z` tag is the **only** manual trigger you need — you do **not** run any workflow by hand. The tag push starts the ["Create a Release"](https://github.com/hyperlight-dev/hyperlight-js/actions/workflows/CreateRelease.yml) workflow ([`CreateRelease.yml`](https://github.com/hyperlight-dev/hyperlight-js/blob/main/.github/workflows/CreateRelease.yml)), which reads the version from the tag and then does everything else automatically:

1. **Creates the release branch** — a `release/v0.18.0` branch is created and pushed for you.
2. **Creates the GitHub release** — a new [GitHub release](https://github.com/hyperlight-dev/hyperlight-js/releases) is published with automatically generated release notes and the benchmark results attached.
3. **Publishes the crates to crates.io** — in dependency order (`hyperlight-js-common` → `hyperlight-js-runtime` → `hyperlight-js`). Verify on the [hyperlight-js page on crates.io](https://crates.io/crates/hyperlight-js).
4. **Publishes the npm packages to npmjs.com** — `@hyperlight-dev/js-host-api` and its platform-specific binary packages, with their versions set from the tag. Verify on the [npmjs.com package page](https://www.npmjs.com/package/@hyperlight-dev/js-host-api).

Both crates.io and npm publishing use trusted publishing (OIDC), so no `NPM_TOKEN` or crates.io token secret is needed for the `CreateRelease` workflow. Provenance attestations are generated automatically for the npm packages.

> **Note:** Only a `vX.Y.Z` **tag** push triggers a real release. Pushing to `main`, or running the workflow manually with **Run workflow**, performs a **dry run** — it builds and validates everything but publishes nothing.

### npm trusted publishing setup

Trusted publishing is configured on [npmjs.com](https://www.npmjs.com/) for each package. If you add a new platform package, you must configure its trusted publisher:

1. Go to the package on npmjs.com → Settings → Trusted Publisher
2. Select **GitHub Actions**
3. Set **Organization**: `hyperlight-dev`, **Repository**: `hyperlight-js`, **Workflow**: `CreateRelease.yml`
4. Save

This must be done for all 4 packages:
- `@hyperlight-dev/js-host-api`
- `@hyperlight-dev/js-host-api-linux-x64-gnu`
- `@hyperlight-dev/js-host-api-linux-x64-musl`
- `@hyperlight-dev/js-host-api-win32-x64-msvc`

> **Note:** Trusted publishing only works via `CreateRelease.yml` (the production release path). Manual publishing is deliberately discouraged — see [Manual npm publishing (emergency only)](#manual-npm-publishing-emergency-only) below.

## Patching a release

If you need to update a previously released version of hyperlight-js then you should open a Pull Request against the release branch you want to patch, for example if you wish to patch the release `v0.18.0` then you should open a PR against the `release/v0.18.0` branch.

Once the PR is merged, then you should follow the instructions above. In this instance the version number of the tag should be a patch version, for example if you are patching the `release/v0.18.0` branch and this is the first patch release to that branch then the tag should be `v0.18.1`. If you are patching a patch release then the tag should be `v0.18.2` and the target branch should be `release/v0.18.1` and so on.

## Manual npm publishing (emergency only)

> ⚠️ **Do not use this for regular releases.** Use the `CreateRelease` workflow instead. Manual publishing bypasses OIDC trusted publishing and will **not** generate provenance attestations — meaning publish will show up without the "Published via trusted publishing" badge on npmjs.com. Only use this if the automated release pipeline is broken and you need to ship an urgent fix.

If you need to publish npm packages manually via `workflow_dispatch`, you'll need to:

1. **Temporarily allow token-based publishing on npmjs.com**
   - Go to each package on [npmjs.com](https://www.npmjs.com/) → Settings → Publishing access
   - Change from "Require two-factor authentication and disallow tokens" to "Require two-factor authentication or automation tokens"
   - Do this for all 4 packages:
     - `@hyperlight-dev/js-host-api`
     - `@hyperlight-dev/js-host-api-linux-x64-gnu`
     - `@hyperlight-dev/js-host-api-linux-x64-musl`
     - `@hyperlight-dev/js-host-api-win32-x64-msvc`

2. **Create an npm automation token**
   - Go to [npmjs.com](https://www.npmjs.com/) → Access Tokens → Generate New Token → Granular Access Token
   - Set scope to the `@hyperlight-dev` org, packages: read and write, expiry: shortest available
   - Copy the token

3. **Add the token as a repo secret**
   - Go to the repo on GitHub → Settings → Secrets and variables → Actions → New repository secret
   - Name: `NPM_TOKEN`
   - Value: paste the token from the previous step
   - Save

4. **Run the workflow**
   - Go to Actions → "Publish npm packages" → Run workflow
   - Select the correct branch
   - Enter the version (e.g. `0.2.1`)
   - Set `dry_run` to `false`

5. **Clean up immediately after publishing**
   - Delete the `NPM_TOKEN` repo secret on GitHub → Settings → Secrets and variables → Actions
   - Revoke the npm token on npmjs.com → Access Tokens
   - Re-enable "Require two-factor authentication and disallow tokens" on all 4 packages
   - Verify the packages published correctly: `npm view @hyperlight-dev/js-host-api versions`
