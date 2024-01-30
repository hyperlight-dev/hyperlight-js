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

Create a PR with these changes and merge it into the `dev` branch.

## Create a tag

When the `dev` branch has reached a state in which you want to release a new Cargo version, you should create a tag. Although you can do this from the GitHub releases page, we currently recommend doing the tag from the command line. Do so with the following commands:

```bash
git tag -a v0.18.0 -m "A brief description of the release"
git push origin v0.18.0 # if you've named your git remote for the hyperlight-dev/hyperlight-js repo differently, change 'origin' to your remote name
```

>Note: we'll use `v0.18.0` as the version for the above and all subsequent instructions. You should replace this with the version you're releasing. Make sure your version follows [SemVer](https://semver.org) conventions as closely as possible, and is prefixed with a `v` character. *In particular do not use a patch version unless you are patching an issue in a release branch, releases from dev should always be minor or major versions*.
If you are creating a patch release see the instructions [here](#patching-a-release).

## Create a release branch (no manual steps)

After you push your new tag in the previous section, the ["Create a Release Branch"](https://github.com/hyperlight-dev/hyperlight-js/blob/dev/.github/workflows/CreateReleaseBranch.yml) CI job will automatically run. When this job completes, a new `release/v0.18.0` branch will be automatically created for you.

## Create a new GitHub release and publish the crates

After the previous CI job runs to create the new release branch, go to the ["Create a Release"](https://github.com/hyperlight-dev/hyperlight-js/actions/workflows/CreateRelease.yml) Github actions workflow and do the following:
1. Click the "Run workflow" button near the top right
1. In the Use workflow from dropdown, select the `release/v0.18.0` branch
1. Click the green **Run workflow** button

When this job is done, a new [GitHub release](https://github.com/hyperlight-dev/hyperlight-js/releases) will be created for you. 

This release contains the benchmark results and the source code for the release along with automatically generated release notes.

In addition the hyperlight-js crates will be published to crates.io. You can verify this by going to the [hyperlight-js page on crates.io](https://crates.io/crates/hyperlight-js) and checking that the new version is listed.

## Patching a release

If you need to update a previously released version of hyperlight-js then you should open a Pull Request against the release branch you want to patch, for example if you wish to patch the release `v0.18.0` then you should open a PR against the `release/v0.18.0` branch.

Once the PR is merged, then you should follow the instructions above. In this instance the version number of the tag should be a patch version, for example if you are patching the `release/v0.18.0` branch and this is the first patch release to that branch then the tag should be `v0.18.1`. If you are patching a patch release then the tag should be `v0.18.2` and the target branch should be `release/v0.18.1` and so on.
