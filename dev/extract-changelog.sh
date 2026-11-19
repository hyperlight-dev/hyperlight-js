#!/usr/bin/env bash
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
  if (p) exit
  if ($0 ~ "^## \\[" ver "\\]") { p=1; next }
}
p' CHANGELOG.md
