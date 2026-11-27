# Create a new hyperlight-js release

This document details the process of releasing a new version of hyperlight-js to [crates.io](https://crates.io/) and [npmjs.com](https://www.npmjs.com/). It's intended to be used as a checklist for the developer doing the release. 

## Update versions

The first step in the release process is to bump the version numbers — the Rust crates **and** the npm packages — keeping them all in sync.

Do this with the `just set-version` recipe. **Always use this instead of bumping by hand** (`cargo set-version`, `npm version`, editing `package.json`): piecemeal bumps are exactly what leaves a lockfile stale and breaks CI mid-release. In one step it updates:

- every workspace crate's `version` and the root `Cargo.lock`,
- the excluded `extended_runtime` fixture's own `Cargo.lock` (a bare `cargo set-version` can't reach it, and a stale one fails the `native_modules --locked` build),
- the npm main package and the six platform packages,
- the root package version in `src/js-host-api/package-lock.json`.

Keep `optionalDependencies` on the latest published version so `npm ci` can use
the lockfile. The release workflow updates them before publishing.

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

> **Note:** The release workflow sets all npm versions from the tag before publishing.

## Prepare the changelog

Like Hyperlight, we maintain a curated [CHANGELOG.md](../CHANGELOG.md) using
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). It supplies the highlights
at the start of release notes; GitHub's generated PR list follows them. Do not
replace the curated entries with generated PR titles.

Before tagging, include these changes in the release-preparation PR:

1. Audit PRs merged since the previous release against the changelog, including
   dependency updates with user-visible effects. Record significant changes and
   explicitly mark **Breaking:** entries with migration instructions for Rust,
   Node.js, custom guests, and persisted snapshots where applicable. Do not rely
   solely on PR labels to find breaking changes.
2. Rename `## [Prerelease] - Unreleased` to, for example,
   `## [v0.18.0] - YYYY-MM-DD`, using the actual release date.
3. Add a new empty `## [Prerelease] - Unreleased` section at the top for the next
   release.
4. Update the comparison links at the bottom, for example:

   ```markdown
   [Prerelease]: https://github.com/hyperlight-dev/hyperlight-js/compare/v0.18.0...HEAD
   [v0.18.0]: https://github.com/hyperlight-dev/hyperlight-js/compare/v0.17.0...v0.18.0
   ```

5. From the repository root, preview the notes with an authenticated
   GitHub CLI:

   ```console
   just create-release-notes v0.18.0
   ```

   Before a version has been prepared, use `just create-release-notes dev-latest`
   to preview the current `Prerelease` section. These commands combine the
   extracted changelog section with GitHub's generate-notes API output; they do
   not publish anything. When working in a fork, set
   `GH_REPO=hyperlight-dev/hyperlight-js` to preview upstream PR notes.

Review the preview for accuracy and completeness, merge the preparation PR, and
tag that commit. The release workflow uses the same recipe to generate the notes
passed to `gh release create --notes-file`. Dry runs use the `Prerelease` section
and publish nothing.

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
2. **Prepares the GitHub release inputs** — benchmark results are built and validated.
3. **Publishes the crates to crates.io** — in dependency order (`hyperlight-js-common` → `hyperlight-js-runtime` → `hyperlight-js`). Verify on the [hyperlight-js page on crates.io](https://crates.io/crates/hyperlight-js).
4. **Publishes the npm packages to npmjs.com** — `@hyperlight-dev/js-host-api` and its platform-specific binary packages, with their versions set from the tag. Verify on the [npmjs.com package page](https://www.npmjs.com/package/@hyperlight-dev/js-host-api).
5. **Publishes the completed GitHub release** — only after npm and crate publishing succeed, the workflow creates or resumes a draft, attaches benchmarks, the three published `.crate` archives, and their provenance bundles, then publishes it with curated changelog highlights followed by GitHub-generated PR notes.

Both crates.io and established npm packages use trusted publishing (OIDC), so they do not need long-lived publishing tokens. A new npm package needs a short-lived `NPM_TOKEN` for its first release, as described below. Provenance attestations are generated for every npm package.

### Immutable releases and provenance

Release immutability is enabled so that publishing locks the release tag and assets.
GitHub automatically generates a release attestation recording the commit and
asset hashes. The workflow requires this attestation: verification fails after
publication if this attestation is missing or invalid. (i.e. if immutable releases are turned off or disabled)

The three crate archives also have signed build provenance linking them to their
source and build workflow. Their provenance bundles are included in the release;
benchmarks do not have separate build provenance. The workflow verifies crate
provenance before publishing and release integrity afterward.

### Release-note labels

GitHub generates the full PR-list portion of release notes from merged
pull-request titles and labels using [`.github/release.yml`](../.github/release.yml).
Commit messages, including conventional-commit `BREAKING CHANGE` footers, are not
used for categorization. Curated highlights come from `CHANGELOG.md`, independently
of these labels.

Any pull request that breaks a released API must have the `breaking-change`
label so it appears under **Breaking Changes**. This is in addition to the
single required `kind/*` label enforced by `PRLabelChecker.yml`. Describe the
affected Rust and Node APIs and their migration in the pull-request body and
the changelog.

> **Note:** Only a `vX.Y.Z` **tag** push triggers a real release. Pushing to `main`, or running the workflow manually with **Run workflow**, performs a **dry run** — it builds and validates everything but publishes nothing.

### npm trusted publishing setup

Trusted publishing is configured on [npmjs.com](https://www.npmjs.com/) for each package. If you add a new platform package, you must configure its trusted publisher:

1. Go to the package on npmjs.com → Settings → Trusted Publisher
2. Select **GitHub Actions**
3. Set **Organization**: `hyperlight-dev`, **Repository**: `hyperlight-js`, **Workflow**: `CreateRelease.yml`
4. Save

This must be done for all 6 packages:
- `@hyperlight-dev/js-host-api`
- `@hyperlight-dev/js-host-api-linux-x64-gnu`
- `@hyperlight-dev/js-host-api-linux-x64-musl`
- `@hyperlight-dev/js-host-api-win32-x64-msvc`
- `@hyperlight-dev/js-host-api-win32-arm64-msvc`
- `@hyperlight-dev/js-host-api-darwin-arm64`
- `@hyperlight-dev/js-host-api-linux-arm64-gnu`

> **Note:** Trusted publishers are configured per package, and npm cannot configure one for a
> package that does not exist yet. The publish workflow therefore has a temporary
> `FIRST_TIME_PACKAGES` list in `.github/workflows/npm-publish.yml`. Packages in that list use
> the `NPM_TOKEN` repository secret for their first release; all other packages continue to use
> trusted publishing (OIDC). Both authentication paths publish with npm provenance.
>
> For a new package, add its full npm name to `FIRST_TIME_PACKAGES`, ensure the short-lived
> `NPM_TOKEN` secret is available, and release normally from the `CreateRelease` workflow. After
> the release, configure the package's GitHub Actions trusted publisher using the settings above,
> delete the `NPM_TOKEN` repository secret, revoke the short-lived npm token, then open a
> follow-up PR that removes the package from `FIRST_TIME_PACKAGES` and regenerates
> `src/js-host-api/package-lock.json` with:
>
> ```console
> cd src/js-host-api
> npm install --package-lock-only --ignore-scripts --os=darwin --cpu=arm64
> ```
>
> Use the target package's platform and architecture for other packages. The regenerated lockfile
> adds the package's `resolved` URL and `integrity` hash. This follow-up PR is required because
> the hash cannot exist until npm has published the package; it is not a release-workflow failure.

> **Note:** Trusted publishing only works via `CreateRelease.yml` (the production release path). Manual publishing is deliberately discouraged — see [Manual npm publishing (emergency only)](#manual-npm-publishing-emergency-only) below.

## Patching a release

If you need to update a previously released version of hyperlight-js then you should open a Pull Request against the release branch you want to patch, for example if you wish to patch the release `v0.18.0` then you should open a PR against the `release/v0.18.0` branch.

Once the PR is merged, then you should follow the instructions above. In this instance the version number of the tag should be a patch version, for example if you are patching the `release/v0.18.0` branch and this is the first patch release to that branch then the tag should be `v0.18.1`. If you are patching a patch release then the tag should be `v0.18.2` and the target branch should be `release/v0.18.1` and so on.

Prepare the patch version's dated changelog section on the branch being tagged.
Include only changes shipped in that patch, retain its existing release history,
and compare the patch tag against the previous patch/base release. Do not copy
unreleased entries from `main` unless those changes are included in the patch.

## Manual npm publishing (emergency only)

> ⚠️ **Do not use this for regular releases.** Use the `CreateRelease` workflow instead. Manual publishing uses `NPM_TOKEN` instead of OIDC trusted publishing. It still generates provenance attestations, but the release will not show the "Published via trusted publishing" badge on npmjs.com. Only use this if the automated release pipeline is broken and you need to ship an urgent fix.

If you need to publish npm packages manually via `workflow_dispatch`, you'll need to:

1. **Temporarily allow token-based publishing on npmjs.com**
   - Go to each package on [npmjs.com](https://www.npmjs.com/) → Settings → Publishing access
   - Change from "Require two-factor authentication and disallow tokens" to "Require two-factor authentication or automation tokens"
   - Do this for all 6 packages:
     - `@hyperlight-dev/js-host-api`
     - `@hyperlight-dev/js-host-api-linux-x64-gnu`
     - `@hyperlight-dev/js-host-api-linux-x64-musl`
     - `@hyperlight-dev/js-host-api-win32-x64-msvc`
     - `@hyperlight-dev/js-host-api-win32-arm64-msvc`
     - `@hyperlight-dev/js-host-api-darwin-arm64`
     - `@hyperlight-dev/js-host-api-linux-arm64-gnu`

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
   - Re-enable "Require two-factor authentication and disallow tokens" on all 6 packages
   - Verify the packages published correctly: `npm view @hyperlight-dev/js-host-api versions`
