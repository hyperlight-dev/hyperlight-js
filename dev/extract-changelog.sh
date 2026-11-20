#!/usr/bin/env bash
# Print one version's release notes from CHANGELOG.md in the current directory,
# excluding its heading. A missing version produces no output and is not an error.
set -Eeuo pipefail

if [[ $# -lt 1 ]]; then
    echo "Usage: $0 <version>"
    echo "  Example: $0 v0.2.0"
    echo "  Example: $0 Prerelease"
    exit 1
fi

version=$1

awk -v ver="$version" '
/^## \[.*\]/ {
  # Stop at the next version heading once the requested section has started.
  if (p) exit
  # Match the version literally; enable printing and skip the heading itself.
  if (index($0, "## [" ver "]") == 1) { p=1; next }
}
# Print each line while inside the requested section (p is the printing flag).
p' CHANGELOG.md
