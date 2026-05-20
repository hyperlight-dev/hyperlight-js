# Create a new hyperlight-js release

This document details the process of releasing a new version of hyperlight-js to [crates.io](https://crates.io/). It's intended to be used as a checklist for the developer doing the release. The checklist is represented in the below sections.

## Update cargo.toml Versions

The first step in the release process is to update the version numbers of the crates you are releasing.

Update the `version` field in the `[workspace.package]` section of the root `Cargo.toml`, as well as the `hyperlight-js-runtime` entry in `[workspace.dependencies]`.

The easiest way to do this is with the `cargo-edit` crate, which provides a `cargo set-version` command. Install it with:

```console
cargo install cargo-edit
```

Then update the version number:

```console
cargo set-version 0.18.0
```

For simplicity, we keep the version number consistent across all crates in the repository.

Create a PR with these changes and merge it into the `main` branch.

## Create a tag

When the `main` branch has reached a state in which you want to release a new Cargo version, you should create a tag. Although you can do this from the GitHub releases page, we currently recommend doing the tag from the command line. Do so with the following commands:

```bash
git tag -a v0.18.0 -m "A brief description of the release"
git push origin v0.18.0 # if you've named your git remote for the hyperlight-dev/hyperlight-js repo differently, change 'origin' to your remote name
```

>Note: we'll use `v0.18.0` as the version for the above and all subsequent instructions. You should replace this with the version you're releasing. Make sure your version follows [SemVer](https://semver.org) conventions as closely as possible, and is prefixed with a `v` character. *In particular do not use a patch version unless you are patching an issue in a release branch, releases from main should always be minor or major versions*.
If you are creating a patch release see the instructions [here](#patching-a-release).

## Create a release branch (no manual steps)

After you push your new tag in the previous section, the ["Create a Release Branch"](https://github.com/hyperlight-dev/hyperlight-js/blob/main/.github/workflows/CreateReleaseBranch.yml) CI job will automatically run. When this job completes, a new `release/v0.18.0` branch will be automatically created for you.

## Create a new GitHub release and publish the crates

After the previous CI job runs to create the new release branch, go to the ["Create a Release"](https://github.com/hyperlight-dev/hyperlight-js/actions/workflows/CreateRelease.yml) Github actions workflow and do the following:
1. Click the "Run workflow" button near the top right
1. In the Use workflow from dropdown, select the `release/v0.18.0` branch
1. Click the green **Run workflow** button

When this job is done, a new [GitHub release](https://github.com/hyperlight-dev/hyperlight-js/releases) will be created for you. 

This release contains the benchmark results and the source code for the release along with automatically generated release notes.

In addition the hyperlight-js crates will be published to crates.io. You can verify this by going to the [hyperlight-js page on crates.io](https://crates.io/crates/hyperlight-js) and checking that the new version is listed.

The npm packages (`@hyperlight-dev/js-host-api` and platform-specific binaries) are also published automatically as part of this workflow. Publishing uses [npm trusted publishing (OIDC)](https://docs.npmjs.com/trusted-publishers) — no `NPM_TOKEN` secret is needed for the `CreateRelease` workflow. Provenance attestations are generated automatically.

You can verify the npm publish by checking the [npmjs.com package page](https://www.npmjs.com/package/@hyperlight-dev/js-host-api).

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
