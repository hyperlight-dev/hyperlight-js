#!/usr/bin/env bash
# Prints PUBLISH_<PACKAGE>=true|false for each npm package, for GITHUB_ENV.
# The variable name drops the scope: @hyperlight-dev/js-host-api becomes
# PUBLISH_JS_HOST_API.
set -euo pipefail

if [ "$#" -lt 2 ]; then
    echo "Usage: $0 VERSION PACKAGE..." >&2
    exit 1
fi

VERSION=$1
shift

response_file=$(mktemp)
trap 'rm -f "$response_file"' EXIT

for package in "$@"; do
    package_env_var=${package#@*/}
    package_env_var=${package_env_var^^}
    package_env_var=PUBLISH_${package_env_var//-/_}

    if ! http_status=$(
        curl --silent --show-error \
            --retry 3 \
            --retry-all-errors \
            --connect-timeout 10 \
            --max-time 30 \
            --output "$response_file" \
            --write-out '%{http_code}' \
            "https://registry.npmjs.org/$package/$VERSION"
    ); then
        echo "Failed to query npm for $package@$VERSION." >&2
        exit 1
    fi

    case "$http_status" in
        200)
            if ! jq -e --arg version "$VERSION" '.version == $version' "$response_file" > /dev/null; then
                echo "npm returned malformed or mismatched version data for $package@$VERSION." >&2
                exit 1
            fi
            echo "${package_env_var}=false"
            echo "$package@$VERSION already exists." >&2
            ;;
        404)
            echo "${package_env_var}=true"
            echo "$package@$VERSION will be published." >&2
            ;;
        *)
            echo "npm returned HTTP $http_status for $package@$VERSION." >&2
            exit 1
            ;;
    esac
done
